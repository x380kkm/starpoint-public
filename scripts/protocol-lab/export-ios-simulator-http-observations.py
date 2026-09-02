# audience: internal
# # export-ios-simulator-http-observations
# 该脚本从 Simulator App 数据容器读取个人服务请求记录, 并输出场景运行期间的脱敏 HTTP 元数据.

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from contextlib import closing
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional


DATABASE_RELATIVE_PATH = Path(
    "Library/Application Support/StarpointPersonalService/personal-service.sqlite3"
)
REQUIRED_REQUESTS = (
    ("POST", "/auth_login"),
    ("POST", "/check_login"),
    ("POST", "/sync_data"),
    ("GET", "/wf/210009_config_20200415.json"),
)
CORE_EXACT_PATHS = frozenset(path for _, path in REQUIRED_REQUESTS)
CORE_PATH_PREFIXES = (
    "/api/index.php/",
    "/android/",
    "/asset/",
    "/assets/",
    "/cdn/",
    "/ios/",
)
CORE_RESOURCE_SUFFIXES = (
    ".bundle",
    ".csv",
    ".manifest",
    ".pack",
    ".zip",
)


# //// 判断请求是否属于游戏协议或资源 [@x380kkm 2026-08-21] ////
def is_core_request_path(path: str) -> bool:
    normalized = path.split("?", 1)[0]
    return (
        normalized in CORE_EXACT_PATHS
        or normalized.startswith(CORE_PATH_PREFIXES)
        or normalized.lower().endswith(CORE_RESOURCE_SUFFIXES)
    )


# //// /判断请求是否属于游戏协议或资源 ////


# //// 读取场景报告的观察配置 [@x380kkm 2026-08-22] ////
def read_scenario_configuration(
    report_path: Path,
) -> tuple[str, tuple[tuple[str, str], ...]]:
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if not isinstance(report, dict):
        raise ValueError("scenario report is not an object")
    started_at = report.get("started_at")
    if not isinstance(started_at, str) or not started_at:
        raise ValueError("scenario report does not contain started_at")
    configured_requests = report.get("required_requests")
    if configured_requests is None:
        return started_at, REQUIRED_REQUESTS
    if not isinstance(configured_requests, list):
        raise ValueError("scenario required_requests is not a list")

    required_requests: list[tuple[str, str]] = []
    for configured_request in configured_requests:
        if not isinstance(configured_request, dict):
            raise ValueError("scenario required request is not an object")
        method = configured_request.get("method")
        path = configured_request.get("path")
        if not isinstance(method, str) or not method.strip():
            raise ValueError("scenario required request does not contain method")
        if not isinstance(path, str) or not path.strip():
            raise ValueError("scenario required request does not contain path")
        request_key = (method.strip().upper(), path.strip().split("?", 1)[0])
        if request_key not in required_requests:
            required_requests.append(request_key)
    return started_at, tuple(required_requests)


# //// /读取场景报告的观察配置 ////


# //// 查询场景运行期间的 HTTP observations [@x380kkm 2026-08-21] ////
def query_observations(database_path: Path, started_at: str) -> list[dict[str, object]]:
    if not database_path.is_file():
        raise FileNotFoundError("personal service database is missing")
    database_uri = f"{database_path.resolve().as_uri()}?mode=ro"
    with closing(sqlite3.connect(database_uri, uri=True, timeout=5)) as connection:
        connection.row_factory = sqlite3.Row
        rows = connection.execute(
            """
            SELECT method, path, status, count, first_seen, last_seen
            FROM http_observations
            WHERE julianday(last_seen) >= julianday(?)
            ORDER BY last_seen, method, path, status
            """,
            (started_at,),
        ).fetchall()
    return [
        {
            "method": row["method"],
            "path": row["path"],
            "status": row["status"],
            "count": row["count"],
            "first_seen": row["first_seen"],
            "last_seen": row["last_seen"],
            "core": is_core_request_path(row["path"]),
        }
        for row in rows
    ]


# //// /查询场景运行期间的 HTTP observations ////


# //// 生成 observations 判定报告 [@x380kkm 2026-08-22] ////
def build_report(
    observations: list[dict[str, object]],
    started_at: str,
    required_requests: tuple[tuple[str, str], ...] = REQUIRED_REQUESTS,
) -> dict[str, object]:
    required_request_keys = set(required_requests)
    core_observations = [observation for observation in observations if observation["core"]]
    core_failures = [
        {
            "method": observation["method"],
            "path": observation["path"],
            "status": observation["status"],
        }
        for observation in observations
        if observation["core"] and not 200 <= int(observation["status"]) < 300
    ]
    required_failures = [
        {
            "method": observation["method"],
            "path": observation["path"],
            "status": observation["status"],
        }
        for observation in observations
        if (
            str(observation["method"]).upper(),
            str(observation["path"]).split("?", 1)[0],
        )
        in required_request_keys
        and not 200 <= int(observation["status"]) < 300
    ]
    observed_request_keys = {
        (
            str(observation["method"]).upper(),
            str(observation["path"]).split("?", 1)[0],
        )
        for observation in observations
    }
    missing_required_requests = [
        {"method": method, "path": path}
        for method, path in required_requests
        if (method, path) not in observed_request_keys
    ]
    if core_failures:
        error_code = "CORE_HTTP_RESPONSE_FAILED"
        first_failure = core_failures[0]
    elif required_failures:
        error_code = "REQUIRED_HTTP_RESPONSE_FAILED"
        first_failure = required_failures[0]
    elif missing_required_requests:
        error_code = "REQUIRED_OBSERVATIONS_MISSING"
        first_failure = {
            "reason": "required_request_missing",
            **missing_required_requests[0],
        }
    else:
        error_code = None
        first_failure = None
    return {
        "format_version": 1,
        "platform": "ios-simulator",
        "started_at": started_at,
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "status": "passed" if error_code is None else "failed",
        "error_code": error_code,
        "first_failure": first_failure,
        "core_observation_count": len(core_observations),
        "core_failure_count": len(core_failures),
        "required_failure_count": len(required_failures),
        "required_failures": required_failures,
        "required_observation_count": (
            len(required_requests) - len(missing_required_requests)
        ),
        "missing_required_requests": missing_required_requests,
        "observations": observations,
    }


# //// /生成 observations 判定报告 ////


# //// 生成 observations 查询失败报告 [@x380kkm 2026-08-22] ////
def build_query_failure_report(started_at: Optional[str]) -> dict[str, object]:
    return {
        "format_version": 1,
        "platform": "ios-simulator",
        "started_at": started_at,
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "status": "failed",
        "error_code": "OBSERVATIONS_QUERY_FAILED",
        "first_failure": None,
        "core_observation_count": None,
        "core_failure_count": None,
        "required_failure_count": None,
        "required_failures": [],
        "required_observation_count": None,
        "missing_required_requests": [],
        "observations": [],
    }


# //// /生成 observations 查询失败报告 ////


# //// 解析命令行参数 [@x380kkm 2026-08-21] ////
def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-container", type=Path, required=True)
    parser.add_argument("--scenario-report", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


# //// /解析命令行参数 ////


# //// 导出 observations 并用退出码返回核心请求状态 [@x380kkm 2026-08-22] ////
def main() -> int:
    arguments = parse_arguments()
    started_at = None
    try:
        started_at, required_requests = read_scenario_configuration(
            arguments.scenario_report
        )
        observations = query_observations(
            arguments.data_container / DATABASE_RELATIVE_PATH,
            started_at,
        )
        report = build_report(observations, started_at, required_requests)
    except (FileNotFoundError, json.JSONDecodeError, sqlite3.Error, ValueError):
        report = build_query_failure_report(started_at)
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(arguments.output)
    return 0 if report["status"] == "passed" else 1


# //// /导出 observations 并用退出码返回核心请求状态 ////


if __name__ == "__main__":
    sys.exit(main())
