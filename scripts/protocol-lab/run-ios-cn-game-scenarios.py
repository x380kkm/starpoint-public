# audience: internal
# # run-ios-cn-game-scenarios
# 该脚本通过 iOS Simulator 内的 loopback 个人服务执行真实 CN MessagePack 游戏流程.
# 报告只包含阶段, HTTP 元数据和聚合断言, 不包含实例身份, 凭据或玩家快照.

import base64
import binascii
import csv
import io
import json
import math
import socket
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlsplit

from ios_cn_game_scenario_stages import (
    STAGE_NAMES,
    ScenarioFailure,
    require_scenario as _require_scenario,
)
from ios_cn_gameplay_scenario_stages import ScenarioStages
from ios_cn_messagepack import pack_messagepack, unpack_messagepack
from ios_loopback_http import (
    InvalidHttpUrl,
    NonLoopbackUrl,
    download as download_loopback,
    normalize_base_url as normalize_loopback_base_url,
    request as request_loopback,
    require_response_urls,
)


SCENARIO_VERSION = 1
_MISSING = object()


# //// 复用本机多人会话的 NUL 分隔 JSON 连接 [@x380kkm 2026-08-25] ////
class _SessionFrames:
    def __init__(self, base_url, timeout_seconds):
        self.host = urlsplit(base_url).hostname
        self.timeout_seconds = timeout_seconds
        self.connections = {}

    def request(self, port, frame, receive_count=1, channel="lobby"):
        key = (int(port), channel)
        if key not in self.connections:
            try:
                stream = socket.create_connection(
                    (self.host, key[0]), self.timeout_seconds
                )
                stream.settimeout(self.timeout_seconds)
            except OSError:
                raise ScenarioFailure(
                    "MULTI_SESSION_CONNECT_FAILED",
                    "the multiplayer loopback session could not be opened",
                )
            self.connections[key] = [stream, bytearray()]
        stream, buffer = self.connections[key]
        if frame is not None:
            try:
                stream.sendall(
                    json.dumps(frame, separators=(",", ":")).encode("utf-8") + b"\0"
                )
            except OSError:
                raise ScenarioFailure(
                    "MULTI_SESSION_WRITE_FAILED",
                    "the multiplayer loopback session could not send a frame",
                )
        frames = [self._receive(stream, buffer) for _ in range(receive_count)]
        return frames[0] if receive_count == 1 else frames

    def _receive(self, stream, buffer):
        try:
            while 0 not in buffer:
                chunk = stream.recv(16 * 1024)
                if not chunk:
                    raise ScenarioFailure(
                        "MULTI_SESSION_CLOSED",
                        "the multiplayer session closed before a complete frame",
                    )
                buffer.extend(chunk)
            separator = buffer.index(0)
            frame = bytes(buffer[:separator])
            del buffer[: separator + 1]
            return json.loads(frame.decode("utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            raise ScenarioFailure(
                "MULTI_SESSION_READ_FAILED",
                "the multiplayer loopback session returned an invalid frame",
            )

    def close(self):
        for stream, _ in self.connections.values():
            stream.close()
        self.connections.clear()
# //// /复用本机多人会话的 NUL 分隔 JSON 连接 ////


# //// 规范化场景服务地址 [@x380kkm 2026-08-21] ////
def normalized_base_url(value):
    try:
        return normalize_loopback_base_url(value)
    except NonLoopbackUrl:
        raise ScenarioFailure(
            "NON_LOOPBACK_BASE_URL",
            "base URL must use a numeric loopback address",
        )
    except InvalidHttpUrl:
        raise ScenarioFailure("INVALID_BASE_URL", "base URL must use HTTP or HTTPS")


# //// 拒绝响应数据中的外部 HTTP URL [@x380kkm 2026-08-21] ////
def require_loopback_response_urls(value):
    try:
        require_response_urls(value)
    except NonLoopbackUrl:
        raise ScenarioFailure(
            "NON_LOOPBACK_RESPONSE_URL",
            "the response contains a non-loopback URL",
        )


# //// 将异常压缩为脱敏错误类别 [@x380kkm 2026-08-21] ////
def stage_failure(error):
    if isinstance(error, ScenarioFailure):
        return {"error_code": error.code, "message": str(error)}
    if isinstance(error, (TimeoutError, socket.timeout)):
        return {
            "error_code": "REQUEST_TIMEOUT",
            "message": "the scenario request timed out",
        }
    return {
        "error_code": "REQUEST_FAILED",
        "message": "the scenario request failed",
    }


# //// 生成与 JavaScript 报告一致的 UTC 时间 [@x380kkm 2026-08-21] ////
def _iso_timestamp():
    value = datetime.now(timezone.utc).isoformat(timespec="milliseconds")
    return value.replace("+00:00", "Z")


# //// 计算阶段耗时 [@x380kkm 2026-08-21] ////
def _elapsed_milliseconds(started_at):
    return max(0, int(time.time() * 1000) - started_at)


# //// 发送一个限制在 loopback 的 HTTP 请求 [@x380kkm 2026-08-21] ////
def _request_http(
    base_url,
    timeout_seconds,
    method,
    target,
    body,
    content_type,
    headers,
    failure_code,
    failure_message,
):
    try:
        return request_loopback(
            base_url,
            timeout_seconds,
            method,
            target,
            body,
            content_type,
            headers,
        )
    except NonLoopbackUrl:
        raise ScenarioFailure(
            "NON_LOOPBACK_RESPONSE_URL",
            "the response contains a non-loopback URL",
        )
    except TimeoutError:
        raise
    except Exception:
        raise ScenarioFailure(failure_code, failure_message)


# //// 解码本地 JSON 响应 [@x380kkm 2026-08-21] ////
def _decode_json_response(status, content_type, body, method, path):
    _require_scenario(
        200 <= status < 300,
        "JSON_HTTP_%d" % status,
        "%s %s returned HTTP %d" % (method, path, status),
    )
    _require_scenario(
        content_type == "application/json",
        "JSON_CONTENT_TYPE_INVALID",
        "%s %s did not return JSON" % (method, path),
    )
    try:
        decoded = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise ScenarioFailure(
            "JSON_BODY_INVALID",
            "%s %s returned invalid JSON" % (method, path),
        )
    require_loopback_response_urls(decoded)
    return decoded


# //// 发送不带管理凭据的本地 JSON 请求 [@x380kkm 2026-08-21] ////
def _request_json(base_url, timeout_seconds, method, path, payload=_MISSING, headers=None):
    if payload is _MISSING:
        body = None
        content_type = None
    else:
        body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        content_type = "application/json"
    response = _request_http(
        base_url,
        timeout_seconds,
        method,
        path,
        body,
        content_type,
        headers,
        "JSON_REQUEST_FAILED",
        "%s %s could not be completed" % (method, path),
    )
    return _decode_json_response(*response, method, path)


# //// 发送雷霆本地游客注册请求 [@x380kkm 2026-08-21] ////
def _request_guest(base_url, timeout_seconds):
    path = "/mobile!guestRegister.action"
    response = _request_http(
        base_url,
        timeout_seconds,
        "POST",
        path,
        b"",
        "application/x-www-form-urlencoded",
        None,
        "JSON_REQUEST_FAILED",
        "POST guest registration could not be completed",
    )
    return _decode_json_response(*response, "POST", path)


# //// 发送 Base64 包装的 CN MessagePack 请求 [@x380kkm 2026-08-21] ////
def _request_cn(base_url, timeout_seconds, path, payload, headers=None):
    response = _request_http(
        base_url,
        timeout_seconds,
        "POST",
        path,
        base64.b64encode(pack_messagepack(payload)),
        "application/x-www-form-urlencoded",
        headers,
        "CN_REQUEST_FAILED",
        "POST %s could not be completed" % path,
    )
    status, content_type, body = response
    _require_scenario(
        200 <= status < 300,
        "CN_HTTP_%d" % status,
        "POST %s returned HTTP %d" % (path, status),
    )
    _require_scenario(
        content_type == "application/x-msgpack",
        "CN_CONTENT_TYPE_INVALID",
        "POST %s did not return MessagePack" % path,
    )
    try:
        packed = base64.b64decode(body.decode("ascii"), validate=True)
        envelope = unpack_messagepack(packed)
    except (UnicodeDecodeError, ValueError, TypeError, binascii.Error):
        raise ScenarioFailure(
            "CN_BODY_INVALID",
            "POST %s returned invalid Base64 MessagePack" % path,
        )
    require_loopback_response_urls(envelope)
    response_headers = envelope.get("data_headers") if isinstance(envelope, dict) else None
    result_code = (
        response_headers.get("result_code")
        if isinstance(response_headers, dict)
        else None
    )
    _require_scenario(
        result_code == 1 and not isinstance(result_code, bool),
        "CN_RESULT_CODE_INVALID",
        "POST %s did not return result_code 1" % path,
    )
    _require_scenario(
        isinstance(envelope, dict) and "data" in envelope,
        "CN_DATA_MISSING",
        "POST %s did not return a data object" % path,
    )
    return envelope


# //// 下载并解析版本接口返回的本地 CSV 清单 [@x380kkm 2026-08-22] ////
def _request_asset_list(base_url, timeout_seconds, location):
    status, content_type, body = _request_http(
        base_url,
        timeout_seconds,
        "GET",
        location,
        None,
        None,
        None,
        "ASSET_LIST_REQUEST_FAILED",
        "the asset entity list request could not be completed",
    )
    _require_scenario(
        200 <= status < 300,
        "ASSET_LIST_HTTP_%d" % status,
        "asset entity list returned HTTP %d" % status,
    )
    _require_scenario(
        content_type == "text/csv",
        "ASSET_LIST_CONTENT_TYPE_INVALID",
        "asset entity list did not return CSV content",
    )
    try:
        decoded = body.decode("utf-8-sig")
        rows = list(csv.reader(io.StringIO(decoded, newline=""), strict=True))
    except (UnicodeDecodeError, csv.Error):
        raise ScenarioFailure(
            "ASSET_LIST_BODY_INVALID",
            "asset entity list returned invalid CSV",
        )
    require_loopback_response_urls(rows)
    return rows


# //// 下载 get_path 返回的本地资产归档 [@x380kkm 2026-08-21] ////
def _request_archive(base_url, timeout_seconds, location):
    try:
        return download_loopback(base_url, timeout_seconds, location)
    except NonLoopbackUrl:
        raise ScenarioFailure(
            "NON_LOOPBACK_RESPONSE_URL",
            "the response contains a non-loopback URL",
        )
    except TimeoutError:
        raise
    except Exception:
        raise ScenarioFailure(
            "ASSET_ARCHIVE_REQUEST_FAILED",
            "the asset archive request could not be completed",
        )


# //// 执行一条严格有序的 CN 游戏场景链 [@x380kkm 2026-08-21] ////
def run_cn_game_scenarios(
    base_url,
    request_timeout_ms=10_000,
    device_id=None,
    sleep_impl=None,
):
    _require_scenario(
        isinstance(request_timeout_ms, int)
        and not isinstance(request_timeout_ms, bool)
        and request_timeout_ms > 0,
        "TIMEOUT_INVALID",
        "--timeout-ms must be a positive integer",
    )
    base_url = normalized_base_url(base_url)
    timeout_seconds = request_timeout_ms / 1000.0
    session_frames = _SessionFrames(base_url, timeout_seconds)
    selected_device_id = (
        device_id
        if device_id is not None
        else int(time.time() * 1000) % 2_000_000_000 + 1
    )
    stages = ScenarioStages(
        request_json=lambda method, path, *payload: _request_json(
            base_url,
            timeout_seconds,
            method,
            path,
            *payload,
        ),
        request_cn=lambda path, payload, headers=None: _request_cn(
            base_url,
            timeout_seconds,
            path,
            payload,
            headers,
        ),
        request_guest=lambda: _request_guest(base_url, timeout_seconds),
        request_asset_list=lambda location: _request_asset_list(
            base_url,
            timeout_seconds,
            location,
        ),
        request_archive=lambda location: _request_archive(
            base_url,
            timeout_seconds,
            location,
        ),
        device_id=selected_device_id,
        sleep_impl=sleep_impl,
        request_session=session_frames.request,
    )
    report = {
        "format_version": SCENARIO_VERSION,
        "platform": "ios-simulator",
        "transport": "base64-msgpack-cn",
        "started_at": _iso_timestamp(),
        "finished_at": None,
        "status": "running",
        "first_failure": None,
        "last_successful_stage": None,
        "stages": [],
    }
    scenarios = {name: getattr(stages, name) for name in STAGE_NAMES}
    for stage_name in STAGE_NAMES:
        if report["first_failure"] is not None:
            report["stages"].append(
                {
                    "name": stage_name,
                    "status": "blocked",
                    "error_code": "UPSTREAM_STAGE_FAILED",
                    "depends_on": report["first_failure"]["stage"],
                }
            )
            continue
        stage_started_at = int(time.time() * 1000)
        try:
            evidence = scenarios[stage_name]()
            report["stages"].append(
                {
                    "name": stage_name,
                    "status": "passed",
                    "duration_ms": _elapsed_milliseconds(stage_started_at),
                    "evidence": evidence,
                }
            )
            report["last_successful_stage"] = stage_name
        except Exception as error:
            failure = stage_failure(error)
            report["first_failure"] = {
                "stage": stage_name,
                "error_code": failure["error_code"],
            }
            report["stages"].append(
                {
                    "name": stage_name,
                    "status": "failed",
                    "duration_ms": _elapsed_milliseconds(stage_started_at),
                    "error_code": failure["error_code"],
                    "message": failure["message"],
                }
            )
    report["status"] = "passed" if report["first_failure"] is None else "failed"
    report["finished_at"] = _iso_timestamp()
    session_frames.close()
    return report


# //// 解析场景命令行参数 [@x380kkm 2026-08-21] ////
def parse_arguments(argv):
    options = {}
    index = 0
    while index < len(argv):
        argument = argv[index]
        _require_scenario(
            argument.startswith("--"),
            "ARGUMENT_INVALID",
            "unexpected argument %s" % argument,
        )
        value = argv[index + 1] if index + 1 < len(argv) else None
        _require_scenario(
            value is not None and not value.startswith("--"),
            "ARGUMENT_VALUE_MISSING",
            "%s requires a value" % argument,
        )
        if argument == "--base-url":
            options["base_url"] = value
        elif argument == "--output":
            options["output"] = value
        elif argument == "--timeout-ms":
            try:
                parsed_timeout = float(value)
            except ValueError:
                parsed_timeout = math.nan
            options["request_timeout_ms"] = (
                int(parsed_timeout)
                if math.isfinite(parsed_timeout) and parsed_timeout.is_integer()
                else parsed_timeout
            )
        else:
            raise ScenarioFailure("ARGUMENT_INVALID", "unknown option %s" % argument)
        index += 2
    _require_scenario(
        isinstance(options.get("base_url"), str),
        "BASE_URL_REQUIRED",
        "--base-url is required",
    )
    if "request_timeout_ms" in options:
        timeout = options["request_timeout_ms"]
        _require_scenario(
            isinstance(timeout, int)
            and not isinstance(timeout, bool)
            and timeout > 0,
            "TIMEOUT_INVALID",
            "--timeout-ms must be a positive integer",
        )
    return options


# //// 执行场景命令并写入报告 [@x380kkm 2026-08-21] ////
def main(argv=None):
    try:
        options = parse_arguments(sys.argv[1:] if argv is None else argv)
        report = run_cn_game_scenarios(
            options["base_url"],
            request_timeout_ms=options.get("request_timeout_ms", 10_000),
        )
        serialized = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        if options.get("output"):
            Path(options["output"]).resolve().write_text(serialized, encoding="utf-8")
        sys.stdout.write(serialized)
        return 0 if report["status"] == "passed" else 1
    except Exception as error:
        failure = stage_failure(error)
        sys.stderr.write("%s: %s\n" % (failure["error_code"], failure["message"]))
        return 1


if __name__ == "__main__":
    sys.exit(main())
