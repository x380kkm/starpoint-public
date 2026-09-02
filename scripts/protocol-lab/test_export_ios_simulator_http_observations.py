# audience: internal
# # test-export-ios-simulator-http-observations
# 该测试验证 Simulator observations 导出范围和核心失败判定.

from __future__ import annotations

import importlib.util
import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("export-ios-simulator-http-observations.py")
SPEC = importlib.util.spec_from_file_location("ios_simulator_observations", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SimulatorObservationTests(unittest.TestCase):
    # //// 构造请求记录数据库 [@x380kkm 2026-08-21] ////
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.database_path = Path(self.temporary_directory.name) / "personal-service.sqlite3"
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
            connection.commit()

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def add_observation(self, path: str, status: int, last_seen: str) -> None:
        with closing(sqlite3.connect(self.database_path)) as connection:
            connection.execute(
                "INSERT INTO http_observations VALUES (?, ?, ?, 1, ?, ?)",
                ("POST", path, status, last_seen, last_seen),
            )
            connection.commit()

    @staticmethod
    def observation(method: str, path: str, status: int = 200) -> dict[str, object]:
        timestamp = "2026-08-21T22:00:01.000Z"
        return {
            "method": method,
            "path": path,
            "status": status,
            "count": 1,
            "first_seen": timestamp,
            "last_seen": timestamp,
            "core": MODULE.is_core_request_path(path),
        }

    def required_observations(self) -> list[dict[str, object]]:
        return [self.observation(method, path) for method, path in MODULE.REQUIRED_REQUESTS]

    # //// /构造请求记录数据库 ////

    # //// 忽略场景开始前的记录 [@x380kkm 2026-08-21] ////
    def test_query_observations_uses_scenario_start(self) -> None:
        self.add_observation("/api/index.php/load", 404, "2026-08-21T21:59:59.000Z")
        self.add_observation("/api/index.php/load", 200, "2026-08-21T22:00:01.000Z")

        observations = MODULE.query_observations(
            self.database_path,
            "2026-08-21T22:00:00.000Z",
        )

        self.assertEqual([observation["status"] for observation in observations], [200])

    # //// /忽略场景开始前的记录 ////

    # //// 解析默认和显式的必需请求集合 [@x380kkm 2026-08-22] ////
    def test_scenario_without_override_keeps_default_required_requests(self) -> None:
        scenario_path = Path(self.temporary_directory.name) / "scenario.json"
        scenario_path.write_text(
            json.dumps({"started_at": "2026-08-21T22:00:00.000Z"}),
            encoding="utf-8",
        )

        started_at, required_requests = MODULE.read_scenario_configuration(
            scenario_path
        )

        self.assertEqual(started_at, "2026-08-21T22:00:00.000Z")
        self.assertEqual(required_requests, MODULE.REQUIRED_REQUESTS)

    def test_explicit_required_requests_replace_default_set(self) -> None:
        scenario_path = Path(self.temporary_directory.name) / "scenario.json"
        scenario_path.write_text(
            json.dumps(
                {
                    "started_at": "2026-08-21T22:00:00.000Z",
                    "required_requests": [
                        {"method": "post", "path": "/sync_data"},
                        {
                            "method": "GET",
                            "path": "/wf/210009_config_20200415.json",
                        },
                    ],
                }
            ),
            encoding="utf-8",
        )

        started_at, required_requests = MODULE.read_scenario_configuration(
            scenario_path
        )
        observations = [
            self.observation(method, path) for method, path in required_requests
        ]
        report = MODULE.build_report(observations, started_at, required_requests)

        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["required_observation_count"], 2)
        self.assertEqual(report["missing_required_requests"], [])

        observations.append(self.observation("POST", "/auth_login", 500))
        report = MODULE.build_report(observations, started_at, required_requests)

        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["error_code"], "CORE_HTTP_RESPONSE_FAILED")
        self.assertEqual(report["missing_required_requests"], [])

    # //// /解析默认和显式的必需请求集合 ////

    # //// 核心非成功响应使报告失败 [@x380kkm 2026-08-22] ////
    def test_core_non_success_response_fails_report(self) -> None:
        observations = self.required_observations()
        observations[1]["status"] = 500

        report = MODULE.build_report(observations, "2026-08-21T22:00:00.000Z")

        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["core_failure_count"], 1)
        self.assertEqual(report["required_observation_count"], 4)
        self.assertEqual(report["missing_required_requests"], [])

    # //// /核心非成功响应使报告失败 ////

    # //// 请求记录缺少必需 POST 时使报告失败 [@x380kkm 2026-08-22] ////
    def test_missing_required_post_fails_report(self) -> None:
        observations = self.required_observations()
        sync_observation = next(
            observation
            for observation in observations
            if observation["path"] == "/sync_data"
        )
        sync_observation["method"] = "GET"

        report = MODULE.build_report(observations, "2026-08-21T22:00:00.000Z")

        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["error_code"], "REQUIRED_OBSERVATIONS_MISSING")
        self.assertEqual(report["required_observation_count"], 3)
        self.assertEqual(
            report["missing_required_requests"],
            [{"method": "POST", "path": "/sync_data"}],
        )

    # //// /请求记录缺少必需 POST 时使报告失败 ////

    # //// 非核心遥测错误保留记录但不阻断 [@x380kkm 2026-08-22] ////
    def test_non_core_sdk_response_is_reported_without_core_failure(self) -> None:
        self.assertFalse(MODULE.is_core_request_path("/wf_crash/crash.php"))
        observations = self.required_observations()
        observations.append(self.observation("POST", "/wf_crash/crash.php", 404))
        report = MODULE.build_report(
            observations,
            "2026-08-21T22:00:00.000Z",
        )

        self.assertEqual(report["status"], "passed")
        self.assertEqual(len(report["observations"]), 5)

    # //// /非核心遥测错误保留记录但不阻断 ////

    # //// 必需 SDK 请求的非成功响应使报告失败 [@x380kkm 2026-08-25] ////
    def test_required_sdk_non_success_response_fails_report(self) -> None:
        sdk_request = ("GET", "/chat-sdk/sdk/user/v2/config.action")
        required_requests = MODULE.REQUIRED_REQUESTS + (sdk_request,)
        observations = [
            self.observation(method, path) for method, path in required_requests
        ]
        observations[-1]["status"] = 404

        report = MODULE.build_report(
            observations,
            "2026-08-21T22:00:00.000Z",
            required_requests,
        )

        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["error_code"], "REQUIRED_HTTP_RESPONSE_FAILED")
        self.assertEqual(report["required_failure_count"], 1)
        self.assertEqual(
            report["required_failures"],
            [{"method": "GET", "path": sdk_request[1], "status": 404}],
        )
        self.assertEqual(report["missing_required_requests"], [])

    # //// /必需 SDK 请求的非成功响应使报告失败 ////

    # //// 查询失败报告保持稳定字段 [@x380kkm 2026-08-22] ////
    def test_query_failure_report_has_report_fields(self) -> None:
        successful_report = MODULE.build_report(
            self.required_observations(),
            "2026-08-21T22:00:00.000Z",
        )

        failed_report = MODULE.build_query_failure_report(None)

        self.assertEqual(set(failed_report), set(successful_report))

    # //// /查询失败报告保持稳定字段 ////


if __name__ == "__main__":
    unittest.main()
