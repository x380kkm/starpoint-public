# audience: internal
# # test-analyze-ios-device-observations
# 该测试验证真机观察报告能区分当前 HTTP 状态, 恢复记录, 传输失败和客户端错误.

from __future__ import annotations

import importlib.util
import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("analyze-ios-device-observations.py")
SPEC = importlib.util.spec_from_file_location("ios_device_observations", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DeviceObservationTests(unittest.TestCase):
    # //// 构造手机诊断目录和观察数据库 [@x380kkm 2026-08-27] ////
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        self.database_path = self.root / "personal-service.sqlite3"
        with closing(sqlite3.connect(self.database_path)) as connection:
            connection.execute(
                """
                CREATE TABLE http_observations (
                    method TEXT NOT NULL,
                    path TEXT NOT NULL,
                    status INTEGER NOT NULL,
                    count INTEGER NOT NULL,
                    first_seen TEXT NOT NULL,
                    last_seen TEXT NOT NULL,
                    PRIMARY KEY (method, path, status)
                )
                """
            )
            connection.executemany(
                "INSERT INTO http_observations VALUES (?, ?, ?, ?, ?, ?)",
                [
                    (
                        "POST",
                        "/api/index.php/load",
                        404,
                        1,
                        "2026-08-27T01:40:00.000Z",
                        "2026-08-27T01:40:00.000Z",
                    ),
                    (
                        "POST",
                        "/api/index.php/load",
                        200,
                        2,
                        "2026-08-27T01:44:00.000Z",
                        "2026-08-27T01:44:01.000Z",
                    ),
                    (
                        "GET",
                        "/health",
                        599,
                        3,
                        "2026-08-27T01:42:00.000Z",
                        "2026-08-27T01:42:00.000Z",
                    ),
                    (
                        "GET",
                        "/health",
                        200,
                        4,
                        "2026-08-27T01:45:00.000Z",
                        "2026-08-27T01:45:00.000Z",
                    ),
                ],
            )
            connection.commit()

        replay = {
            "k": [
                2,
                {
                    "logicStatus": {
                        "errorMessage": [
                            {
                                "code": "C8013",
                                "internalMessage": "odds table missing",
                                "debugInfo": {
                                    "date": "2026/08/27_09:44:11.193",
                                    "stackTrace": "[ClientError]:8013:odds table missing",
                                },
                            }
                        ]
                    }
                },
            ]
        }
        (self.root / "latest-replay.log").write_text(
            json.dumps(replay, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        info = {
            "date": "2026/08/27_09:44:12.000",
            "code": "M12",
            "error": "正常系自动发送",
        }
        (self.root / "latest-info.json").write_text(
            json.dumps(info, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        sobot = {
            "title": "[2026-08-27 12:33:58.949]网络请求失败",
            "content": "http://00@127.1:17171/chat-sdk/sdk/user/v2/appInit.action errorThe resource could not be loaded because the App Transport Security policy requires the use of a secure connection.",
            "time": "1787805238000",
        }
        (self.root / "SobotLog-20260827.txt").write_text(
            json.dumps(sobot, ensure_ascii=False) + ",\n", encoding="utf-8"
        )

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    # //// /构造手机诊断目录和观察数据库 ////

    # //// 当前状态覆盖同路径历史失败 [@x380kkm 2026-08-27] ////
    def test_collapses_http_history_to_current_status(self) -> None:
        local_timezone = MODULE.parse_timezone("+08:00")
        observations = MODULE.read_http_observations(
            self.database_path,
            MODULE.parse_timestamp("2026-08-27T01:39:00Z", local_timezone),
            None,
            local_timezone,
        )
        statuses = MODULE.collapse_http_statuses(observations, local_timezone)

        self.assertEqual(len(statuses), 2)
        load = next(item for item in statuses if item["path"].endswith("/load"))
        self.assertEqual(load["current_status"], 200)
        self.assertEqual(load["history"][1]["status"], 404)
        health = next(item for item in statuses if item["path"] == "/health")
        self.assertEqual(health["current_status"], 200)
        self.assertEqual(health["recovered_transport_statuses"][0]["status"], 599)

    # //// /当前状态覆盖同路径历史失败 ////

    # //// 分离当前 HTTP 错误和 599 传输失败 [@x380kkm 2026-08-27] ////
    def test_classifies_current_and_transport_failures(self) -> None:
        local_timezone = MODULE.parse_timezone("+08:00")
        observations = MODULE.read_http_observations(self.database_path)
        statuses = MODULE.collapse_http_statuses(observations, local_timezone)
        current, transport, recovered, recovered_transport = MODULE.classify_http_statuses(
            statuses
        )

        self.assertEqual(current, [])
        self.assertEqual(transport, [])
        self.assertEqual(len(recovered), 1)
        self.assertEqual(len(recovered_transport), 1)

    # //// /分离当前 HTTP 错误和 599 传输失败 ////

    # //// 报告分别统计 400, 500 和 599 [@x380kkm 2026-08-27] ////
    def test_reports_http_errors_separately_from_transport_failures(self) -> None:
        local_timezone = MODULE.parse_timezone("+08:00")
        observations = MODULE.read_http_observations(self.database_path)
        observations.extend(
            [
                {
                    "method": "POST",
                    "path": "/api/index.php/tutorial/update_step",
                    "status": 400,
                    "count": 2,
                    "first_seen": "2026-08-27T02:00:00.000Z",
                    "last_seen": "2026-08-27T02:00:01.000Z",
                    "scope": "client",
                    "core": True,
                },
                {
                    "method": "POST",
                    "path": "/api/index.php/asset/get_path",
                    "status": 500,
                    "count": 3,
                    "first_seen": "2026-08-27T02:01:00.000Z",
                    "last_seen": "2026-08-27T02:01:01.000Z",
                    "scope": "client",
                    "core": True,
                },
                {
                    "method": "GET",
                    "path": "/sync_data",
                    "status": 599,
                    "count": 1,
                    "first_seen": "2026-08-27T02:02:00.000Z",
                    "last_seen": "2026-08-27T02:02:00.000Z",
                    "scope": "client",
                    "core": True,
                },
            ]
        )
        statuses = MODULE.collapse_http_statuses(observations, local_timezone)
        report = MODULE.build_report(
            observations,
            statuses,
            [],
            None,
            None,
            local_timezone,
        )

        self.assertEqual(report["http_current_error_count"], 2)
        self.assertEqual(report["http_current_failure_count"], 3)
        self.assertEqual(report["transport_current_failure_count"], 1)
        self.assertEqual(
            {item["current_status"] for item in report["http_current_errors"]},
            {400, 500},
        )
        self.assertEqual(report["transport_failures"][0]["current_status"], 599)

    # //// /报告分别统计 400, 500 和 599 ////

    # //// 解析 AIR 和 ATS 客户端错误 [@x380kkm 2026-08-27] ////
    def test_reads_client_runtime_and_sobot_errors(self) -> None:
        local_timezone = MODULE.parse_timezone("+08:00")
        errors = MODULE.read_client_errors(self.root, local_timezone)
        normalized = MODULE.normalize_client_errors(
            errors,
            MODULE.parse_timestamp("2026-08-27T01:43:52Z", local_timezone),
            None,
            local_timezone,
        )

        self.assertEqual([error["code"] for error in normalized], ["ATS", "C8013"])
        self.assertIn("odds table missing", normalized[1]["message"])
        ats = normalized[0]
        self.assertEqual(ats["kind"], "sdk_client")
        self.assertEqual(ats["category"], "ats_pre_request")
        self.assertTrue(ats["blocked_before_http"])

    # //// /解析 AIR 和 ATS 客户端错误 ////

    # //// 基线只保留新增累计记录和新客户端错误 [@x380kkm 2026-08-27] ////
    def test_baseline_filters_unchanged_records(self) -> None:
        local_timezone = MODULE.parse_timezone("+08:00")
        baseline_observations = MODULE.read_http_observations(self.database_path)
        current_observations = [dict(item) for item in baseline_observations]
        recovered = next(
            item
            for item in current_observations
            if item["path"] == "/api/index.php/load" and item["status"] == 200
        )
        recovered["count"] += 1
        recovered["last_seen"] = "2026-08-27T02:00:00.000Z"
        transport = next(
            item
            for item in current_observations
            if item["path"] == "/health" and item["status"] == 599
        )
        transport["count"] += 1
        transport["last_seen"] = "2026-08-27T02:01:00.000Z"
        current_observations.append(
            {
                "method": "GET",
                "path": "/asset/missing.bundle",
                "status": 404,
                "count": 1,
                "first_seen": "2026-08-27T02:02:00.000Z",
                "last_seen": "2026-08-27T02:02:00.000Z",
                "scope": "client",
                "core": True,
            }
        )
        selected = MODULE.select_http_observations(
            current_observations,
            baseline_observations,
            None,
            None,
            local_timezone,
        )

        self.assertEqual(len(selected), 3)
        self.assertTrue(all(item["observed_count"] == 1 for item in selected))
        statuses = MODULE.collapse_http_statuses(
            current_observations,
            selected,
            None,
            local_timezone,
        )
        current, transport_rows, recovered_rows, _ = MODULE.classify_http_statuses(
            statuses
        )
        self.assertEqual([item["path"] for item in current], ["/asset/missing.bundle"])
        self.assertEqual([item["path"] for item in transport_rows], ["/health"])
        self.assertEqual([item["path"] for item in recovered_rows], ["/api/index.php/load"])

        existing = MODULE.read_client_errors(self.root, local_timezone)
        new_error = dict(existing[0])
        new_error["code"] = "F1000"
        new_error["message"] = "resource archive is invalid"
        new_error["stack_trace"] = "[FatalError]:1000:resource archive is invalid"
        new_error["occurred_at"] = "2026-08-27T02:00:00.000Z"
        normalized = MODULE.normalize_client_errors(
            [*existing, new_error],
            None,
            None,
            local_timezone,
            baseline_errors=existing,
        )
        self.assertEqual([item["code"] for item in normalized], ["F1000"])

    # //// /基线只保留新增累计记录和新客户端错误 ////


if __name__ == "__main__":
    unittest.main()
