# audience: external
# # analyze-ios-device-observations
# 此模块合并 iPhone 容器中的 HTTP 记录, AIR 客户端错误和 SDK 本地日志.
# 无时区的游戏日志按 --local-timezone 解释, 基线目录按累计计数筛选新增记录.

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from collections import defaultdict
from contextlib import closing
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Iterable


CLIENT_ERROR_CODE_PATTERN = re.compile(r"^[A-Z][0-9]{3,4}$")
STACK_ERROR_CODE_PATTERN = re.compile(r"\[(Client|Game|Fatal)Error\]:(\d{3,4})")
NORMAL_CLIENT_CODES = {"M12"}
ATS_ERROR_PATTERN = re.compile(
    r"App Transport Security|requires the use of a secure connection",
    re.IGNORECASE,
)
SOBOT_FAILURE_PATTERN = re.compile(
    r"网络请求失败|请求失败|失败|exception|error(?:\b|The|:)",
    re.IGNORECASE,
)
REQUEST_URL_PATTERN = re.compile(r"https?://[^\s{]+", re.IGNORECASE)
LOCAL_DATE_FORMATS = (
    "%Y/%m/%d_%H:%M:%S.%f",
    "%Y-%m-%d %H:%M:%S.%f",
    "%Y-%m-%d %H:%M:%S",
)
SOURCE_DATE_PATTERN = re.compile(
    r"(?P<year>\d{4})_(?P<month>\d{2})_(?P<day>\d{2})_"
    r"(?P<hour>\d{2})_(?P<minute>\d{2})_(?P<second>\d{2})_"
    r"(?P<millisecond>\d{3})"
)
CORE_PATH_PREFIXES = (
    "/api/index.php/",
    "/android/",
    "/asset/",
    "/assets/",
    "/cdn/",
    "/ios/",
)
CORE_PATHS = {"/auth_login", "/check_login", "/health", "/sync_data"}
CORE_PATH_SUFFIXES = (".bundle", ".csv", ".manifest", ".pack", ".zip")
TRANSPORT_FAILURE_STATUS = 599


# //// 解析观察窗口时间 [@x380kkm 2026-08-27] ////
def parse_timezone(value: str) -> timezone:
    match = re.fullmatch(r"([+-])(\d{2}):(\d{2})", value)
    if match is None:
        raise ValueError("--local-timezone 必须使用 +08:00 形式.")
    sign = 1 if match.group(1) == "+" else -1
    minutes = sign * (int(match.group(2)) * 60 + int(match.group(3)))
    return timezone(timedelta(minutes=minutes))


def parse_timestamp(value: object, local_timezone: timezone) -> datetime | None:
    if isinstance(value, int | float):
        seconds = float(value)
        if seconds > 10_000_000_000:
            seconds /= 1000
        return datetime.fromtimestamp(seconds, timezone.utc)
    if not isinstance(value, str) or not value.strip():
        return None
    text = value.strip()
    if re.fullmatch(r"\d+(?:\.\d+)?", text):
        seconds = float(text)
        if seconds > 10_000_000_000:
            seconds /= 1000
        return datetime.fromtimestamp(seconds, timezone.utc)
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(text)
        return parsed.replace(tzinfo=local_timezone) if parsed.tzinfo is None else parsed
    except ValueError:
        pass
    title_match = re.search(r"\[(\d{4}-\d{2}-\d{2} [^\]]+)\]", text)
    if title_match is not None:
        text = title_match.group(1)
    for date_format in LOCAL_DATE_FORMATS:
        try:
            return datetime.strptime(text, date_format).replace(tzinfo=local_timezone)
        except ValueError:
            continue
    return None


def format_timestamp(value: datetime | None) -> str | None:
    if value is None:
        return None
    normalized = value.astimezone(timezone.utc)
    return normalized.isoformat(timespec="milliseconds").replace("+00:00", "Z")
# //// /解析观察窗口时间 ////


# //// 读取 HTTP 累计记录并选择本次观察 [@x380kkm 2026-08-27] ////
def request_scope(path: str) -> str:
    if path.startswith("/manage/"):
        return "management_asset"
    if path.startswith("/v1/"):
        return "management_api"
    return "client"


def is_core_request_path(path: str) -> bool:
    return (
        path in CORE_PATHS
        or path.startswith(CORE_PATH_PREFIXES)
        or path.endswith(CORE_PATH_SUFFIXES)
    )


def _read_all_http_observations(database_path: Path) -> list[dict[str, Any]]:
    if not database_path.is_file():
        return []
    database_uri = database_path.resolve().as_uri() + "?mode=ro"
    with closing(sqlite3.connect(database_uri, uri=True)) as connection:
        rows = connection.execute(
            "SELECT method, path, status, count, first_seen, last_seen "
            "FROM http_observations "
            "ORDER BY last_seen DESC, method, path, status"
        ).fetchall()
    return [
        {
            "method": str(method),
            "path": str(path),
            "status": int(status),
            "count": int(count),
            "first_seen": str(first_seen),
            "last_seen": str(last_seen),
            "scope": request_scope(str(path)),
            "core": is_core_request_path(str(path)),
        }
        for method, path, status, count, first_seen, last_seen in rows
    ]


def read_http_observations(
    database_path: Path,
    started_at: datetime | None = None,
    ended_at: datetime | None = None,
    local_timezone: timezone | None = None,
) -> list[dict[str, Any]]:
    """读取 HTTP 记录, 保留旧的时间窗口调用形式."""
    observations = _read_all_http_observations(database_path)
    if started_at is None and ended_at is None:
        return observations
    if local_timezone is None:
        local_timezone = timezone.utc
    return select_http_observations(
        observations,
        None,
        started_at,
        ended_at,
        local_timezone,
    )


def observation_key(observation: dict[str, Any]) -> tuple[str, str, int]:
    return (
        str(observation["method"]),
        str(observation["path"]),
        int(observation["status"]),
    )


def baseline_observation_index(
    observations: Iterable[dict[str, Any]],
) -> dict[tuple[str, str, int], dict[str, Any]]:
    return {observation_key(item): item for item in observations}


def count_since_baseline(
    current: dict[str, Any],
    previous: dict[str, Any] | None,
    local_timezone: timezone,
) -> int:
    if previous is None:
        return int(current["count"])
    current_count = int(current["count"])
    previous_count = int(previous["count"])
    if current_count > previous_count:
        return current_count - previous_count
    current_last_seen = parse_timestamp(current.get("last_seen"), local_timezone)
    previous_last_seen = parse_timestamp(previous.get("last_seen"), local_timezone)
    if (
        current_last_seen is not None
        and previous_last_seen is not None
        and current_last_seen > previous_last_seen
    ):
        return current_count
    return 0


def select_http_observations(
    observations: Iterable[dict[str, Any]],
    baseline: Iterable[dict[str, Any]] | None,
    started_at: datetime | None,
    ended_at: datetime | None,
    local_timezone: timezone,
) -> list[dict[str, Any]]:
    baseline_index = (
        baseline_observation_index(baseline) if baseline is not None else None
    )
    selected: list[dict[str, Any]] = []
    for observation in observations:
        last_seen = parse_timestamp(observation.get("last_seen"), local_timezone)
        if started_at is not None and (last_seen is None or last_seen < started_at):
            continue
        if ended_at is not None and (last_seen is None or last_seen > ended_at):
            continue
        selected_observation = dict(observation)
        if baseline_index is not None:
            observed_count = count_since_baseline(
                observation,
                baseline_index.get(observation_key(observation)),
                local_timezone,
            )
            if observed_count == 0:
                continue
            selected_observation["observed_count"] = observed_count
            selected_observation["selection"] = "baseline"
        else:
            selected_observation["observed_count"] = None
            selected_observation["selection"] = (
                "time" if started_at is not None else "all"
            )
        selected.append(selected_observation)
    return selected
# //// /读取 HTTP 累计记录并选择本次观察 ////


# //// 折叠 HTTP 当前状态与恢复历史 [@x380kkm 2026-08-27] ////
def status_sort_key(
    observation: dict[str, Any], local_timezone: timezone
) -> datetime:
    return parse_timestamp(
        observation.get("last_seen"), local_timezone
    ) or datetime.min.replace(tzinfo=timezone.utc)


def is_http_error_status(status: int) -> bool:
    return status >= 400 and status != TRANSPORT_FAILURE_STATUS


def collapse_http_statuses(
    observations: Iterable[dict[str, Any]],
    selected_observations: Iterable[dict[str, Any]] | timezone | None = None,
    ended_at: datetime | None = None,
    local_timezone: timezone | None = None,
) -> list[dict[str, Any]]:
    if isinstance(selected_observations, timezone):
        local_timezone = selected_observations
        selected_observations = None
    if local_timezone is None:
        local_timezone = timezone.utc
    source_observations = list(observations)
    selected = (
        list(selected_observations)
        if selected_observations is not None
        else source_observations
    )
    selected_keys = {(item["method"], item["path"]) for item in selected}
    selected_statuses = {observation_key(item): item for item in selected}
    groups: defaultdict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for observation in source_observations:
        last_seen = parse_timestamp(observation.get("last_seen"), local_timezone)
        if ended_at is not None and last_seen is not None and last_seen > ended_at:
            continue
        key = (observation["method"], observation["path"])
        if key in selected_keys:
            groups[key].append(observation)
    collapsed: list[dict[str, Any]] = []
    for (method, path), rows in groups.items():
        ordered = sorted(
            rows,
            key=lambda row: status_sort_key(row, local_timezone),
            reverse=True,
        )
        current = ordered[0]
        history: list[dict[str, Any]] = []
        for row in ordered:
            selected_row = selected_statuses.get(observation_key(row))
            history.append(
                {
                    "status": row["status"],
                    "count": row["count"],
                    "observed_count": (
                        selected_row.get("observed_count")
                        if selected_row is not None
                        else 0
                    ),
                    "first_seen": row["first_seen"],
                    "last_seen": row["last_seen"],
                }
            )
        recovered_http = [
            item
            for item in history
            if is_http_error_status(int(item["status"]))
        ]
        recovered_transport = [
            item
            for item in history
            if int(item["status"]) == TRANSPORT_FAILURE_STATUS
        ]
        current_status = int(current["status"])
        current_state = (
            "transport_failure"
            if current_status == TRANSPORT_FAILURE_STATUS
            else "http_error"
            if is_http_error_status(current_status)
            else "recovered"
            if history
            and any(
                is_http_error_status(int(item["status"]))
                or int(item["status"]) == TRANSPORT_FAILURE_STATUS
                for item in history
            )
            else "ok"
        )
        collapsed.append(
            {
                "method": method,
                "path": path,
                "current_status": current_status,
                "current_state": current_state,
                "current_count": current["count"],
                "first_seen": min(row["first_seen"] for row in ordered),
                "last_seen": current["last_seen"],
                "scope": current["scope"],
                "core": current["core"],
                "history": history,
                "recovered_http_statuses": (
                    recovered_http if current_status < 400 else []
                ),
                "recovered_transport_statuses": (
                    recovered_transport if current_status < 400 else []
                ),
            }
        )
    return sorted(
        collapsed,
        key=lambda row: status_sort_key(row, local_timezone),
        reverse=True,
    )


def classify_http_statuses(
    statuses: Iterable[dict[str, Any]],
) -> tuple[
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    rows = list(statuses)
    current_http_errors = [
        item
        for item in rows
        if is_http_error_status(int(item["current_status"]))
    ]
    transport_failures = [
        item
        for item in rows
        if int(item["current_status"]) == TRANSPORT_FAILURE_STATUS
    ]
    historical_recoveries = [
        item for item in rows if item["recovered_http_statuses"]
    ]
    recovered_transport_failures = [
        item for item in rows if item["recovered_transport_statuses"]
    ]
    return (
        current_http_errors,
        transport_failures,
        historical_recoveries,
        recovered_transport_failures,
    )
# //// /折叠 HTTP 当前状态与恢复历史 ////


# //// 提取 AIR 客户端内部错误 [@x380kkm 2026-08-27] ////
def parse_embedded_error(value: object) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if not isinstance(value, str):
        return {}
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def source_timestamp(path: Path, local_timezone: timezone) -> datetime | None:
    match = SOURCE_DATE_PATTERN.search(path.as_posix())
    if match is not None:
        return datetime(
            int(match.group("year")),
            int(match.group("month")),
            int(match.group("day")),
            int(match.group("hour")),
            int(match.group("minute")),
            int(match.group("second")),
            int(match.group("millisecond")) * 1000,
            tzinfo=local_timezone,
        )
    return None


def normalize_error_code(value: object, stack_trace: str) -> str:
    code = str(value).strip() if value is not None else ""
    stack_match = STACK_ERROR_CODE_PATTERN.search(stack_trace)
    if not code and stack_match is not None:
        prefix = {"Client": "C", "Game": "G", "Fatal": "F"}[
            stack_match.group(1)
        ]
        return prefix + stack_match.group(2)
    if code.isdigit() and stack_match is not None:
        prefix = {"Client": "C", "Game": "G", "Fatal": "F"}[
            stack_match.group(1)
        ]
        return prefix + code
    return code


def client_error_from_entry(
    entry: dict[str, Any],
    source: Path,
    input_root: Path,
    local_timezone: timezone,
) -> dict[str, Any] | None:
    debug = entry.get("debugInfo")
    debug = debug if isinstance(debug, dict) else {}
    embedded = parse_embedded_error(entry.get("error") or debug.get("error"))
    stack_trace = entry.get("stackTrace") or debug.get("stackTrace")
    stack_trace = stack_trace if isinstance(stack_trace, str) else ""
    raw_code = entry.get("code") or debug.get("code") or embedded.get("code")
    code = normalize_error_code(raw_code, stack_trace)
    if code in NORMAL_CLIENT_CODES:
        return None
    message = (
        entry.get("internalMessage")
        or debug.get("internalMessage")
        or embedded.get("internalMessage")
        or embedded.get("message")
        or entry.get("message")
        or debug.get("message")
    )
    if not message and stack_trace:
        message = stack_trace.splitlines()[0]
    message = str(message or "")
    if (
        not stack_trace
        and not message
        and not CLIENT_ERROR_CODE_PATTERN.fullmatch(code)
    ):
        return None
    occurred_at = parse_timestamp(
        entry.get("date")
        or debug.get("date")
        or entry.get("startDate")
        or debug.get("startDate")
        or embedded.get("date"),
        local_timezone,
    ) or source_timestamp(source, local_timezone)
    return {
        "kind": "client_runtime",
        "code": code or "CLIENT",
        "message": message,
        "stack_trace": stack_trace,
        "occurred_at": format_timestamp(occurred_at),
        "api_id": str(entry.get("apiId") or debug.get("apiId") or ""),
        "source": source.relative_to(input_root).as_posix(),
    }


def walk_error_messages(value: object) -> Iterable[dict[str, Any]]:
    if isinstance(value, dict):
        error_messages = value.get("errorMessage")
        if isinstance(error_messages, list):
            for entry in error_messages:
                if isinstance(entry, dict):
                    yield entry
        for key, child in value.items():
            if key != "errorMessage":
                yield from walk_error_messages(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk_error_messages(child)


def read_json_documents(path: Path) -> Iterable[object]:
    text = path.read_text(encoding="utf-8", errors="replace")
    try:
        yield json.loads(text)
        return
    except json.JSONDecodeError:
        pass
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            yield json.loads(stripped)
        except json.JSONDecodeError:
            continue


def read_air_client_errors(
    input_root: Path,
    local_timezone: timezone,
) -> list[dict[str, Any]]:
    names = {
        "latest.log",
        "latest-replay.log",
        "replay.log",
        "info.json",
        "latest-info.json",
    }
    candidates = [
        path
        for path in input_root.rglob("*")
        if path.is_file() and path.name.lower() in names
    ]
    errors: list[dict[str, Any]] = []
    for path in candidates:
        for document in read_json_documents(path):
            for entry in walk_error_messages(document):
                error = client_error_from_entry(
                    entry, path, input_root, local_timezone
                )
                if error is not None:
                    errors.append(error)
            if isinstance(document, dict):
                direct = client_error_from_entry(
                    document, path, input_root, local_timezone
                )
                if direct is not None:
                    errors.append(direct)
    return errors
# //// /提取 AIR 客户端内部错误 ////


# //// 提取 Sobot 和 ATS 请求前阻断 [@x380kkm 2026-08-27] ////
def concatenated_json_objects(text: str) -> Iterable[dict[str, Any]]:
    decoder = json.JSONDecoder()
    offset = 0
    while offset < len(text):
        while offset < len(text) and (text[offset].isspace() or text[offset] == ","):
            offset += 1
        if offset >= len(text):
            break
        try:
            value, end = decoder.raw_decode(text, offset)
        except json.JSONDecodeError:
            offset += 1
            continue
        offset = end
        if isinstance(value, dict):
            yield value


def request_url(value: str) -> str:
    match = REQUEST_URL_PATTERN.search(value)
    return match.group(0) if match is not None else ""


def read_sobot_errors(
    input_root: Path,
    local_timezone: timezone,
) -> list[dict[str, Any]]:
    errors: list[dict[str, Any]] = []
    candidates = [
        path
        for path in input_root.rglob("*.txt")
        if "sobotlog" in path.as_posix().lower()
        and "analysis" not in path.name.lower()
    ]
    for path in candidates:
        text = path.read_text(encoding="utf-8", errors="replace")
        for document in concatenated_json_objects(text):
            title = str(document.get("title") or "")
            content = str(document.get("content") or "")
            combined = f"{title}\n{content}"
            is_ats = ATS_ERROR_PATTERN.search(combined) is not None
            if not is_ats and SOBOT_FAILURE_PATTERN.search(combined) is None:
                continue
            occurred_at = parse_timestamp(
                document.get("time") or title,
                local_timezone,
            ) or source_timestamp(path, local_timezone)
            errors.append(
                {
                    "kind": "sdk_client",
                    "category": "ats_pre_request" if is_ats else "sdk_failure",
                    "code": "ATS" if is_ats else "SDK",
                    "message": content.strip() or title,
                    "stack_trace": "",
                    "occurred_at": format_timestamp(occurred_at),
                    "request_url": request_url(content),
                    "blocked_before_http": is_ats,
                    "source": path.relative_to(input_root).as_posix(),
                }
            )
    return errors
# //// /提取 Sobot 和 ATS 请求前阻断 ////


# //// 合并, 过滤并写出观察报告 [@x380kkm 2026-08-27] ////
def client_error_identity(error: dict[str, Any]) -> tuple[str, str, str, str, str]:
    return (
        str(error.get("kind") or ""),
        str(error.get("code") or ""),
        str(error.get("occurred_at") or ""),
        str(error.get("message") or ""),
        str(error.get("stack_trace") or ""),
    )


def client_error_fingerprint(error: dict[str, Any]) -> tuple[str, str, str, str, str]:
    return (
        str(error.get("kind") or ""),
        str(error.get("code") or ""),
        str(error.get("message") or ""),
        str(error.get("stack_trace") or ""),
        str(error.get("request_url") or error.get("api_id") or ""),
    )


def error_sort_key(error: dict[str, Any], local_timezone: timezone) -> datetime:
    return parse_timestamp(error.get("occurred_at"), local_timezone) or datetime.min.replace(
        tzinfo=timezone.utc
    )


def normalize_client_errors(
    errors: Iterable[dict[str, Any]],
    started_at: datetime | None,
    ended_at: datetime | None,
    local_timezone: timezone,
    baseline_errors: Iterable[dict[str, Any]] = (),
) -> list[dict[str, Any]]:
    baseline = list(baseline_errors)
    baseline_identities = {client_error_identity(error) for error in baseline}
    baseline_undated = {
        client_error_fingerprint(error)
        for error in baseline
        if not error.get("occurred_at")
    }
    unique: dict[tuple[str, str, str, str, str], dict[str, Any]] = {}
    for error in errors:
        identity = client_error_identity(error)
        if identity in baseline_identities or (
            not error.get("occurred_at")
            and client_error_fingerprint(error) in baseline_undated
        ):
            continue
        occurred_at = parse_timestamp(error.get("occurred_at"), local_timezone)
        if started_at is not None and (
            occurred_at is None or occurred_at < started_at
        ):
            continue
        if ended_at is not None and (
            occurred_at is None or occurred_at > ended_at
        ):
            continue
        unique.setdefault(identity, error)
    return sorted(
        unique.values(),
        key=lambda error: error_sort_key(error, local_timezone),
        reverse=True,
    )


def read_client_errors(
    input_root: Path, local_timezone: timezone
) -> list[dict[str, Any]]:
    return [
        *read_air_client_errors(input_root, local_timezone),
        *read_sobot_errors(input_root, local_timezone),
    ]


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def build_report(
    observations: list[dict[str, Any]],
    statuses: list[dict[str, Any]],
    client_errors: list[dict[str, Any]],
    started_at: datetime | None,
    ended_at: datetime | None,
    baseline_root: Path | timezone | None = None,
    local_timezone: timezone | None = None,
) -> dict[str, Any]:
    if isinstance(baseline_root, timezone):
        local_timezone = baseline_root
        baseline_root = None
    if local_timezone is None:
        local_timezone = timezone.utc
    (
        current_http_errors,
        transport_failures,
        historical_recoveries,
        recovered_transport_failures,
    ) = classify_http_statuses(statuses)
    blocking_http_errors = [
        error for error in current_http_errors if error["core"]
    ]
    blocking_transport_failures = [
        error for error in transport_failures if error["core"]
    ]
    internal_errors = [
        error for error in client_errors if error["kind"] == "client_runtime"
    ]
    ats_pre_request_errors = [
        error
        for error in client_errors
        if error.get("category") == "ats_pre_request"
    ]
    timeline: list[dict[str, Any]] = []
    for error in current_http_errors:
        timeline.append(
            {
                "kind": "http",
                "code": str(error["current_status"]),
                "message": f"{error['method']} {error['path']}",
                "occurred_at": error["last_seen"],
            }
        )
    for error in transport_failures:
        timeline.append(
            {
                "kind": "transport",
                "code": str(TRANSPORT_FAILURE_STATUS),
                "message": f"{error['method']} {error['path']}",
                "occurred_at": error["last_seen"],
            }
        )
    timeline.extend(client_errors)
    timeline.sort(
        key=lambda error: error_sort_key(error, local_timezone), reverse=True
    )
    has_blocking_error = bool(
        blocking_http_errors or blocking_transport_failures or internal_errors
    )
    has_warning = bool(current_http_errors or transport_failures or client_errors)
    selection_mode = (
        "baseline"
        if baseline_root is not None
        else "time"
        if started_at is not None
        else "all"
    )
    observed_counts = [item.get("observed_count") for item in observations]
    observed_request_count = (
        sum(int(count) for count in observed_counts)
        if observed_counts and all(count is not None for count in observed_counts)
        else None
    )
    return {
        "schema_version": 2,
        "status": "failed" if has_blocking_error else "warning" if has_warning else "passed",
        "generated_at": format_timestamp(datetime.now(timezone.utc)),
        "selection_mode": selection_mode,
        "baseline_root": str(baseline_root) if baseline_root is not None else None,
        "started_at": format_timestamp(started_at),
        "ended_at": format_timestamp(ended_at),
        "http_observation_count": len(observations),
        "http_request_count": sum(int(item["count"]) for item in observations),
        "http_observed_request_count": observed_request_count,
        "http_current_error_count": len(current_http_errors),
        "http_protocol_error_count": len(current_http_errors),
        "http_current_failure_count": len(current_http_errors) + len(transport_failures),
        "http_blocking_error_count": len(blocking_http_errors)
        + len(blocking_transport_failures),
        "http_recovered_count": len(historical_recoveries),
        "transport_failure_count": len(transport_failures),
        "transport_current_failure_count": len(transport_failures),
        "transport_recovered_count": len(recovered_transport_failures),
        "client_error_count": len(client_errors),
        "client_internal_error_count": len(internal_errors),
        "client_blocking_error_count": len(internal_errors),
        "ats_pre_request_count": len(ats_pre_request_errors),
        "http_current_errors": current_http_errors,
        "http_current_failures": [*current_http_errors, *transport_failures],
        "http_historical_recoveries": historical_recoveries,
        "transport_failures": transport_failures,
        "transport_historical_recoveries": recovered_transport_failures,
        "client_errors": client_errors,
        "latest_failures": timeline[:20],
    }
# //// /合并, 过滤并写出观察报告 ////


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="合并 iPhone 容器中的 HTTP, AIR 和 SDK 错误记录."
    )
    parser.add_argument("input_root", type=Path)
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--baseline-root", "--baseline", dest="baseline_root", type=Path)
    parser.add_argument("--started-at")
    parser.add_argument("--ended-at")
    parser.add_argument("--local-timezone", default="+08:00")
    return parser.parse_args()


def resolved_database_path(root: Path) -> Path:
    return root if root.is_file() else root / "personal-service.sqlite3"


def main() -> None:
    args = parse_args()
    input_root = args.input_root.resolve()
    output_root = (args.output_root or input_root).resolve()
    baseline_root = args.baseline_root.resolve() if args.baseline_root else None
    if baseline_root is not None and not baseline_root.exists():
        raise FileNotFoundError(f"基线目录不存在: {baseline_root}")
    local_timezone = parse_timezone(args.local_timezone)
    started_at = parse_timestamp(args.started_at, local_timezone)
    if args.started_at and started_at is None:
        raise ValueError("--started-at 不是有效时间.")
    ended_at = parse_timestamp(args.ended_at, local_timezone)
    if args.ended_at and ended_at is None:
        raise ValueError("--ended-at 不是有效时间.")
    if started_at is not None and ended_at is not None and ended_at < started_at:
        raise ValueError("--ended-at 必须晚于 --started-at.")

    all_observations = read_http_observations(
        resolved_database_path(input_root)
    )
    baseline_observations = (
        read_http_observations(resolved_database_path(baseline_root))
        if baseline_root is not None
        else None
    )
    observations = select_http_observations(
        all_observations,
        baseline_observations,
        started_at,
        ended_at,
        local_timezone,
    )
    statuses = collapse_http_statuses(
        all_observations,
        observations,
        ended_at,
        local_timezone,
    )
    baseline_client_errors = (
        read_client_errors(baseline_root, local_timezone)
        if baseline_root is not None and baseline_root.is_dir()
        else []
    )
    client_errors = normalize_client_errors(
        read_client_errors(input_root, local_timezone),
        started_at,
        ended_at,
        local_timezone,
        baseline_errors=baseline_client_errors,
    )
    report = build_report(
        observations,
        statuses,
        client_errors,
        started_at,
        ended_at,
        baseline_root,
        local_timezone,
    )
    (
        current_http_errors,
        transport_failures,
        historical_recoveries,
        recovered_transport_failures,
    ) = classify_http_statuses(statuses)
    write_json(output_root / "http-observations.json", observations)
    write_json(output_root / "http-status.json", statuses)
    write_json(output_root / "http-errors.json", current_http_errors)
    write_json(output_root / "http-current-errors.json", current_http_errors)
    write_json(output_root / "http-recovered-statuses.json", historical_recoveries)
    write_json(output_root / "transport-failures.json", transport_failures)
    write_json(
        output_root / "transport-recovered-statuses.json",
        recovered_transport_failures,
    )
    write_json(output_root / "client-errors.json", client_errors)
    write_json(output_root / "ios-device-observations.json", report)
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
