# audience: internal
# # test-run-ios-cn-game-scenarios
# 该测试使用本地 mock HTTP 服务验证 Python iOS CN 场景链的顺序, 编码, 失败定位和报告脱敏.

import base64
import hashlib
import importlib.util
import io
import json
import re
import socketserver
import threading
import unittest
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


RUNNER_PATH = Path(__file__).with_name("run-ios-cn-game-scenarios.py")
RUNNER_SPEC = importlib.util.spec_from_file_location("ios_cn_game_scenarios", RUNNER_PATH)
RUNNER = importlib.util.module_from_spec(RUNNER_SPEC)
RUNNER_SPEC.loader.exec_module(RUNNER)

VIEWER_ID = 987654321
SCENARIO_GACHA_ID = 80000
SCENARIO_EQUIPMENT_GACHA_ID = 3
LOGIN_TOKEN = "scenario-login-token-must-not-leak"
EXTERNAL_URL = "https://must-not-leak.invalid/private?token=response-secret"
DEFAULT_ENTITY_LIST_DIRECTORY = "EntityLists"
IOS_TITLE_ENTITY_LIST_NAME = "10939-ios_medium.csv"
GAME_ENTITY_LIST_NAME = "empty.csv"
FULL_ARCHIVE_PATHS = (
    "/patch/cn/archive-common-full/scenario-shared.zip",
    "/patch/cn/archive-ios-full/scenario-primary.zip",
)
DIFF_ARCHIVE_PATHS = ("/patch/cn/archive-ios-diff/scenario-update.zip",)


# //// 构造可被真实 ZIP 读取器识别的资产归档 [@x380kkm 2026-08-21] ////
def _zip_fixture(name):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_STORED) as archive:
        archive.writestr(
            "%s.txt" % name,
            ("starpoint simulator %s" % name).encode("utf-8"),
        )
    return buffer.getvalue()


# //// 构造 CN MessagePack 响应封套 [@x380kkm 2026-08-21] ////
def _response_envelope(data):
    return {
        "data_headers": {
            "result_code": 1,
            "viewer_id": VIEWER_ID,
            "servertime": 1893542460,
            "force_update": False,
            "asset_update": False,
            "short_udid": 0,
        },
        "data": data,
    }


# //// 投影 mock 玩家载入数据 [@x380kkm 2026-08-21] ////
def _player_load(state):
    return {
        "user_info": {
            **state["user_info"],
            "exp_pool": state["exp_pool"],
        },
        "user_character_list": {
            str(character_id): {
                "character_id": character_id,
                "exp": state["character_exp"] if character_id == 1 else 0,
            }
            for character_id in state["characters"]
        },
        "item_list": {"13": state["item13"], "14001": state["item14001"]},
        "gacha_info_list": [
            {
                "gacha_id": SCENARIO_GACHA_ID,
                "gacha_exchange_point": state["gacha_points"],
                "is_daily_first": True,
            }
        ],
        "quest_progress": (
            {"1": [{"quest_id": 1001002}]}
            if state["quest_finished"]
            else {}
        ),
        "user_tutorial": state["user_tutorial"],
    }


# //// 返回场景 mock 响应 [@x380kkm 2026-08-21] ////
class _ScenarioHandler(BaseHTTPRequestHandler):
    def log_message(self, format_string, *arguments):
        return

    def do_GET(self):
        self._handle()

    def do_POST(self):
        self._handle()

    def _send(self, status, content_type, body):
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_json(self, status, body):
        encoded = json.dumps(body, separators=(",", ":")).encode("utf-8")
        self._send(status, "application/json", encoded)

    def _send_cn(self, data):
        packed = RUNNER.pack_messagepack(_response_envelope(data))
        self._send(200, "application/x-msgpack", base64.b64encode(packed))

    def _send_redirect(self, location):
        self.send_response(302)
        self.send_header("location", location)
        self.send_header("content-length", "0")
        self.end_headers()

    def _read_body(self):
        length = int(self.headers.get("content-length") or 0)
        return self.rfile.read(length)

    def _record_request(self, path, body):
        decoded = None
        if path.startswith("/api/index.php/"):
            decoded = RUNNER.unpack_messagepack(
                base64.b64decode(body.decode("ascii"), validate=True)
            )
        self.server.requests.append(
            {
                "method": self.command,
                "path": path,
                "content_type": self.headers.get("content-type"),
                "headers": {
                    name.lower(): value for name, value in self.headers.items()
                },
                "decoded": decoded,
            }
        )
        return decoded

    def _archive_metadata(self, path):
        archive = self.server.archives[path]
        port = self.server.server_address[1]
        return {
            "location": "http://127.0.0.1:%d%s" % (port, path),
            "size": len(archive),
            "sha256": base64.b64encode(hashlib.sha256(archive).digest()).decode(
                "ascii"
            ),
        }

    def _handle(self):
        body = self._read_body()
        path = urlsplit(self.path).path
        decoded = self._record_request(path, body)
        state = self.server.state

        if path == self.server.fail_path:
            self._send_json(404, {"error": "missing_scenario"})
            return
        if self.command == "GET" and path in self.server.archives:
            if self.server.redirect_archive_external:
                self._send_redirect(EXTERNAL_URL)
            else:
                self._send(200, "application/zip", self.server.archives[path])
            return
        if self.command == "GET" and path in self.server.entity_list_paths:
            self._send(
                200,
                "text/csv; charset=utf-8",
                b"path,size\nscenario/shared,1\n",
            )
            return
        if self.command == "GET" and path == "/wf/210009_config_20200415.json":
            self._send_json(
                200,
                {
                    "token": "0123456789abcdef0123456789abcdef",
                    "config": "00112233445566778899aabbccddeeff",
                },
            )
            return
        if self.command == "GET" and path == "/health":
            port = self.server.server_address[1]
            response_url = "http://127.0.0.1:%d/management" % port
            if self.server.response_url_stage == "health":
                response_url = EXTERNAL_URL
            self._send_json(
                200,
                {
                    "status": "ok",
                    "generation": 0,
                    "links": [{"href": response_url}],
                },
            )
            return
        if self.command == "POST" and path in (
            "/auth_login",
            "/check_login",
            "/mobile!guestRegister.action",
        ):
            encoded_identity = base64.b64encode(
                b'{"userId":"10000001","token":"mock-session-token-0001"}'
            ).decode("ascii")
            self._send_json(
                200,
                {"status": "0", "type": "0", "message": "", "data": encoded_identity},
            )
            return
        if self.command == "POST" and path == "/sync_data":
            self._send_json(200, {"code": 0})
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/channels/channel_leiting/leiting_login"
        ):
            self._send_cn(
                {
                    "status": "success",
                    "userId": decoded.get("userId", ""),
                    "data": {"idCard": "123456", "age": 18, "isGuest": 0, "auth": 1},
                    "online_server_check": True,
                    "heart_beat_interval": 240,
                }
            )
            return
        if self.command == "POST" and path == "/api/index.php/tool/signup":
            data = {
                "login_token": LOGIN_TOKEN,
                "new_account": 1,
                "role_name": "Player",
                "account_name": "Player",
            }
            if self.server.response_url_stage == "signup":
                data["links"] = [{"href": EXTERNAL_URL}]
            self._send_cn(data)
            return
        if self.command == "POST" and path == "/api/index.php/load":
            self._send_cn(_player_load(state))
            return
        if self.command == "POST" and path == "/api/index.php/tutorial/update_step":
            self._send_cn(
                {
                    "step": 12 if decoded.get("skip") is True else 1,
                    "mail_arrived": True,
                    "start_time": 1893542460,
                }
            )
            return
        if self.command == "POST" and path in (
            "/api/index.php/assetintitle/version_info_in_title",
            "/api/index.php/asset/version_info",
        ):
            port = self.server.server_address[1]
            is_title = path == "/api/index.php/assetintitle/version_info_in_title"
            directory = (
                self.server.title_entity_list_directory
                if is_title
                else self.server.game_entity_list_directory
            )
            entity_list_name = (
                IOS_TITLE_ENTITY_LIST_NAME if is_title else GAME_ENTITY_LIST_NAME
            )
            self._send_cn(
                {
                    "base_url": "http://127.0.0.1:%d/patch/cn/%s/"
                    % (port, directory),
                    "files_list": "http://127.0.0.1:%d/patch/cn/%s/%s"
                    % (port, directory, entity_list_name),
                    "total_size": sum(
                        len(archive) for archive in self.server.archives.values()
                    ),
                    "delayed_assets_size": 0,
                }
            )
            return
        if self.command == "POST" and path == "/api/index.php/asset/get_path":
            self._send_cn(
                {
                    "info": {
                        "client_asset_version": self.headers.get("res_ver", ""),
                        "target_asset_version": "1.4.8",
                        "eventual_target_asset_version": "1.4.8",
                        "is_initial": True,
                        "latest_maj_first_version": "1.4.0",
                    },
                    "full": {
                        "version": "1.4.0",
                        "archive": [
                            self._archive_metadata(path) for path in FULL_ARCHIVE_PATHS
                        ],
                    },
                    "diff": [
                        {
                            "version": "1.4.8",
                            "archive": [
                                self._archive_metadata(path)
                                for path in DIFF_ARCHIVE_PATHS
                            ],
                        }
                    ],
                    "asset_version_hash": "",
                }
            )
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/channels/channel_leiting_pay/query_purcharge"
        ):
            self._send_cn({"status": 3})
            return
        if self.command == "POST" and path == "/api/index.php/Pass_card/get_pass_card":
            self._send_cn({"point": 0, "is_buy": False, "all_received_record": []})
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/episode_trial_reading/finish"
        ):
            self._send_cn({})
            return
        if self.command == "GET" and path == "/v1/time":
            state["time_reads"] += 1
            self._send_json(
                200,
                {
                    "enabled": True,
                    "rate": 1,
                    "unix_time_ms": 1893542460000 + state["time_reads"] * 25,
                    "iso": "2030-01-02T00:01:00.000Z",
                },
            )
            return
        if self.command == "POST" and path == "/api/index.php/single_battle_quest/start":
            state["user_info"]["last_main_quest_id"] = 1001002
            self._send_cn(
                {"category_id": 1, "user_info": dict(state["user_info"])}
            )
            return
        if self.command == "POST" and path == "/api/index.php/single_battle_quest/finish":
            state["user_info"]["free_vmoney"] += 30
            state["user_info"]["free_mana"] += 27
            state["user_info"]["rank_point"] = 13
            state["item13"] = 10
            state["character_exp"] = 23
            state["quest_finished"] = True
            self._send_cn(
                {
                    "clear_rank": 5,
                    "before_rank_point": 10,
                    "user_info": dict(state["user_info"]),
                    "rewards": {
                        "reward_pool_exp": 13,
                        "reward_mana": 20,
                        "field_mana": 7,
                    },
                    "item_list": {"13": state["item13"]},
                    "drop_score_reward_ids": [
                        {"group_id": 40000, "index": 1, "number": 10}
                    ],
                    "character_list": [
                        {"character_id": 1, "exp": state["character_exp"]}
                    ],
                }
            )
            return
        if self.command == "POST" and path == "/v1/mails":
            mail = json.loads(body.decode("utf-8"))
            mail["id"] = state["next_mail_id"]
            mail["received"] = False
            state["next_mail_id"] += 1
            state["mails"].append(mail)
            self._send_json(201, {"id": mail["id"]})
            return
        if self.command == "POST" and path == "/api/index.php/mail/receive_all":
            for mail in state["mails"]:
                if mail["received"]:
                    continue
                rewards = mail.get("rewards", {})
                state["user_info"]["free_vmoney"] += int(
                    rewards.get("freeVmoney", 0)
                )
                state["user_info"]["free_mana"] += int(
                    rewards.get("freeMana", 0)
                )
                mail["received"] = True
            self._send_cn(
                {
                    "user_info": dict(state["user_info"]),
                    "mail_ids": [mail["id"] for mail in state["mails"]],
                    "total_count": 0,
                }
            )
            return
        if self.command == "POST" and path == "/api/index.php/gacha/exec":
            if int(decoded["gacha_id"]) == SCENARIO_EQUIPMENT_GACHA_ID:
                state["user_info"]["free_vmoney"] -= 75
                state["equipment_ids"].add(900001)
                self._send_cn(
                    {
                        "draw_equipment": [{"equipment_id": 900001, "seed": 1}],
                        "equipment_list": [{"equipment_id": 900001, "level": 1}],
                        "item_list": {"14001": state["item14001"]},
                        "user_info": dict(state["user_info"]),
                        "gacha_info_list": [{"gacha_id": SCENARIO_EQUIPMENT_GACHA_ID}],
                    }
                )
                return
            is_multi = int(decoded["type"]) == 2
            draw_count = 10 if is_multi else 1
            cost = 1500 if is_multi else 150
            state["user_info"]["free_vmoney"] -= cost
            state["gacha_points"] += draw_count
            draws = [
                {
                    "character_id": 111001 if index % 2 == 0 else 111002,
                    "seed": index + 1,
                }
                for index in range(draw_count)
            ]
            for draw in draws:
                if draw["character_id"] in state["characters"]:
                    state["item14001"] += 1
                    draw["ex_boost_item"] = {"id": 14001, "count": 1}
                else:
                    state["characters"].add(draw["character_id"])
            state["exp_pool"] += len(draws)
            self._send_cn(
                {
                    "user_info": {**state["user_info"], "exp_pool": state["exp_pool"]},
                    "draw": draws,
                    "character_list": [
                        {"character_id": draw["character_id"], "entry_count": 1}
                        for draw in draws
                    ],
                    "gacha_info_list": [
                        {
                            "gacha_id": SCENARIO_GACHA_ID,
                            "gacha_exchange_point": state["gacha_points"],
                            "is_daily_first": True,
                        }
                    ],
                    "item_list": {"14001": state["item14001"]},
                    "ex_boost_item": {"14001": state["item14001"]},
                }
            )
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/multi_battle_quest/create_room"
        ):
            self._send_cn(
                {
                    "access_token": "scenario-room-access",
                    "room_number": "123456",
                    "room_url": "",
                }
            )
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/multi_battle_quest/select_room"
        ):
            self._send_cn(
                {
                    "category_id": 1,
                    "quest_id": 1001002,
                    "room_number": "123456",
                    "raising_state": 1,
                    "ip_address": "127.0.0.1",
                    "port": self.server.session_port,
                }
            )
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/multi_battle_quest/summon"
        ):
            self._send_cn(
                {
                    "mate1": {
                        "com_id": 1,
                        "degree_id": 1,
                        "rank": 1,
                        "party": {"characters": [], "equipments": []},
                    },
                    "mate2": {
                        "com_id": 2,
                        "degree_id": 1,
                        "rank": 1,
                        "party": {"characters": [], "equipments": []},
                    },
                }
            )
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/multi_battle_quest/start"
        ):
            state["user_info"]["last_main_quest_id"] = 1001002
            self._send_cn(
                {
                    "user_info": dict(state["user_info"]),
                    "category_id": 1,
                    "is_multi": "multi",
                    "start_time": 1893542460,
                    "quest_name": "",
                    "follow_bonus_info": None,
                    "client_checks": None,
                    "play_id": decoded["play_id"],
                }
            )
            return
        if (
            self.command == "POST"
            and path == "/api/index.php/multi_battle_quest/finish"
        ):
            self._send_cn(
                {
                    "is_multi": "multi",
                    "host_finished": True,
                    "user_info": dict(state["user_info"]),
                }
            )
            return
        if self.command == "POST" and path == "/api/index.php/mail/index":
            pending = [mail for mail in state["mails"] if not mail["received"]]
            self._send_cn(
                {
                    "total_count": len(pending),
                    "mail": [
                        {"id": mail["id"], "title": mail["title"]}
                        for mail in pending
                    ],
                }
            )
            return
        if self.command == "POST" and path == "/api/index.php/mail/receive":
            mail_id = int(decoded["mail_id"])
            for mail in state["mails"]:
                if mail["id"] != mail_id or mail["received"]:
                    continue
                rewards = mail.get("rewards", {})
                state["user_info"]["free_vmoney"] += int(
                    rewards.get("freeVmoney", 0)
                )
                state["user_info"]["free_mana"] += int(
                    rewards.get("freeMana", 0)
                )
                mail["received"] = True
            self._send_cn(
                {
                    "user_info": dict(state["user_info"]),
                    "total_count": sum(
                        1 for mail in state["mails"] if not mail["received"]
                    ),
                }
            )
            return
        if self.command == "GET" and path == "/v1/activities/catalog":
            self._send_json(
                200,
                {
                    "manifest_state": "loaded",
                    "total": 1,
                    "activities": [
                        {
                            "activity_id": "ranking:1",
                            "status": state["activity_status"],
                        },
                        {
                            "activity_id": "gacha:80000",
                            "name": "Scenario stars gacha",
                            "kind": "gacha",
                            "status": "open",
                            "banner_key": "generated_gacha_banner_80000.png",
                            "banner_width": 510,
                            "banner_height": 180,
                        },
                        {
                            "activity_id": "gacha:3",
                            "name": "装备扭蛋",
                            "kind": "gacha",
                            "status": "open",
                            "banner_key": "generated_gacha_banner_3.png",
                            "banner_width": 510,
                            "banner_height": 180,
                        },
                    ],
                },
            )
            return
        if self.command == "POST" and path == "/v1/activities/ranking%3A1/close":
            state["activity_status"] = "disabled"
            self._send_json(200, {"status": "disabled"})
            return
        if self.command == "POST" and path == "/v1/activities/ranking%3A1/open":
            state["activity_status"] = "open"
            self._send_json(200, {"status": "open"})
            return
        if self.command == "POST" and path == "/v1/checkpoint":
            self._send_json(200, {"status": "ok"})
            return
        self._send_json(404, {"error": "not_found"})


# //// 提供最小真实多人 NUL JSON 会话 [@x380kkm 2026-08-25] ////
class _SessionHandler(socketserver.StreamRequestHandler):
    def handle(self):
        channel = None
        while True:
            frame = self.rfile.read(1)
            if not frame:
                return
            data = bytearray(frame)
            while data[-1] != 0:
                chunk = self.rfile.read(1)
                if not chunk:
                    return
                data.extend(chunk)
            payload = json.loads(bytes(data[:-1]).decode("utf-8"))
            if isinstance(payload, dict):
                channel = payload.get("socklet")
                if channel == "cooperation_battle":
                    response = [0, payload.get("roomNumber", "123456"), ""]
                else:
                    response = [0, "a" * 32, payload.get("roomNumber", "123456")]
            elif isinstance(payload, list) and len(payload) > 1:
                command = payload[1]
                kind = command[0] if isinstance(command, list) and command else None
                if channel == "cooperation_battle" and kind == 0:
                    response = [1, [1]]
                elif channel == "cooperation_battle" and kind == 2:
                    response = [1, [2]]
                elif kind == 0:
                    response = [1, [0, {}, []]]
                elif kind == 99:
                    continue
                elif kind == 4:
                    response = [1, [11, "a" * 32]]
                elif kind == 10:
                    responses = [
                        [1, [1, []]],
                        [1, [2, "123456-npc-1", [1]]],
                        [1, [2, "123456-npc-2", [1]]],
                        [1, [2, "a" * 32, [1]]],
                        [1, [10, 2]],
                    ]
                    for response in responses:
                        self.wfile.write(json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\0")
                    self.wfile.flush()
                    continue
                elif kind == 6:
                    response = [1, [5, []]]
                else:
                    response = [1, [11, "a" * 32]]
            else:
                response = [1, [0]]
            self.wfile.write(json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\0")
            self.wfile.flush()
# //// /提供最小真实多人 NUL JSON 会话 ////


# //// 创建可注入失败的 CN 场景 mock 服务 [@x380kkm 2026-08-21] ////
class _ScenarioServer:
    def __init__(
        self,
        fail_path=None,
        response_url_stage=None,
        redirect_archive_external=False,
        entity_list_directory=DEFAULT_ENTITY_LIST_DIRECTORY,
        game_entity_list_directory=None,
    ):
        self.http_server = ThreadingHTTPServer(
            ("127.0.0.1", 0),
            _ScenarioHandler,
        )
        self.http_server.daemon_threads = True
        self.http_server.fail_path = fail_path
        self.http_server.response_url_stage = response_url_stage
        self.http_server.redirect_archive_external = redirect_archive_external
        self.http_server.title_entity_list_directory = entity_list_directory
        self.http_server.game_entity_list_directory = (
            game_entity_list_directory or entity_list_directory
        )
        self.http_server.entity_list_paths = {
            "/patch/cn/%s/%s"
            % (self.http_server.title_entity_list_directory, IOS_TITLE_ENTITY_LIST_NAME),
            "/patch/cn/%s/%s"
            % (self.http_server.game_entity_list_directory, GAME_ENTITY_LIST_NAME),
        }
        self.session_server = socketserver.ThreadingTCPServer(
            ("127.0.0.1", 0), _SessionHandler
        )
        self.session_server.daemon_threads = True
        self.http_server.session_port = self.session_server.server_address[1]
        self.http_server.archives = {
            path: _zip_fixture("archive-%d" % index)
            for index, path in enumerate(FULL_ARCHIVE_PATHS + DIFF_ARCHIVE_PATHS)
        }
        self.http_server.requests = []
        self.http_server.state = {
            "user_info": {
                "free_vmoney": 150,
                "free_mana": 1000,
                "rank_point": 10,
                "last_main_quest_id": 0,
            },
            "characters": {1},
            "character_exp": 0,
            "exp_pool": 0,
            "item13": 0,
            "item14001": 0,
            "equipment_ids": set(),
            "gacha_points": 0,
            "quest_finished": False,
            "user_tutorial": {"tutorial_step": 0, "skip_flag": False},
            "mails": [],
            "next_mail_id": 700,
            "time_reads": 0,
            "activity_status": "open",
        }
        self.thread = threading.Thread(
            target=self.http_server.serve_forever,
            daemon=True,
        )
        self.session_thread = threading.Thread(
            target=self.session_server.serve_forever,
            daemon=True,
        )

    @property
    def base_url(self):
        return "http://127.0.0.1:%d" % self.http_server.server_address[1]

    @property
    def requests(self):
        return self.http_server.requests

    def __enter__(self):
        self.thread.start()
        self.session_thread.start()
        return self

    def __exit__(self, exception_type, exception, traceback):
        self.http_server.shutdown()
        self.http_server.server_close()
        self.thread.join(timeout=5)
        self.session_server.shutdown()
        self.session_server.server_close()
        self.session_thread.join(timeout=5)


# //// 验证完整顺序, 真实编码和脱敏报告 [@x380kkm 2026-08-21] ////
class ScenarioRunnerTests(unittest.TestCase):
    def test_complete_scenario_chain_and_redacted_report(self):
        with _ScenarioServer() as server:
            report = RUNNER.run_cn_game_scenarios(
                server.base_url,
                device_id=57,
                sleep_impl=lambda milliseconds: None,
            )

        self.assertEqual(report["status"], "passed")
        self.assertIsNone(report["first_failure"])
        self.assertEqual(report["last_successful_stage"], "load_persistence")
        self.assertEqual(
            list(report),
            [
                "format_version",
                "platform",
                "transport",
                "started_at",
                "finished_at",
                "status",
                "first_failure",
                "last_successful_stage",
                "stages",
            ],
        )
        self.assertEqual(
            [stage["name"] for stage in report["stages"]],
            list(RUNNER.STAGE_NAMES),
        )
        self.assertTrue(all(stage["status"] == "passed" for stage in report["stages"]))
        self.assertEqual(
            ["%s %s" % (record["method"], record["path"]) for record in server.requests],
            [
                "GET /health",
                "POST /auth_login",
                "POST /check_login",
                "POST /sync_data",
                "GET /wf/210009_config_20200415.json",
                "POST /mobile!guestRegister.action",
                "POST /api/index.php/channels/channel_leiting/leiting_login",
                "POST /api/index.php/tool/signup",
                "POST /api/index.php/load",
                "POST /api/index.php/tutorial/update_step",
                "POST /api/index.php/assetintitle/version_info_in_title",
                "GET /patch/cn/EntityLists/10939-ios_medium.csv",
                "POST /api/index.php/asset/version_info",
                "GET /patch/cn/EntityLists/empty.csv",
                "POST /api/index.php/asset/get_path",
                "GET /patch/cn/archive-common-full/scenario-shared.zip",
                "GET /patch/cn/archive-ios-full/scenario-primary.zip",
                "GET /patch/cn/archive-ios-diff/scenario-update.zip",
                "POST /api/index.php/channels/channel_leiting_pay/query_purcharge",
                "POST /api/index.php/Pass_card/get_pass_card",
                "POST /api/index.php/episode_trial_reading/finish",
                "GET /v1/time",
                "GET /v1/time",
                "POST /api/index.php/single_battle_quest/start",
                "POST /api/index.php/single_battle_quest/finish",
                "POST /v1/mails",
                "POST /api/index.php/mail/receive_all",
                "POST /api/index.php/load",
                "GET /v1/activities/catalog",
                "POST /api/index.php/load",
                "POST /api/index.php/gacha/exec",
                "POST /api/index.php/gacha/exec",
                "POST /api/index.php/gacha/exec",
                "POST /api/index.php/multi_battle_quest/create_room",
                "POST /api/index.php/multi_battle_quest/select_room",
                "POST /api/index.php/multi_battle_quest/summon",
                "POST /api/index.php/multi_battle_quest/start",
                "POST /api/index.php/multi_battle_quest/finish",
                "POST /v1/mails",
                "POST /api/index.php/mail/index",
                "POST /api/index.php/mail/receive",
                "GET /v1/activities/catalog",
                "POST /v1/activities/ranking%3A1/close",
                "POST /v1/activities/ranking%3A1/open",
                "POST /v1/checkpoint",
                "POST /api/index.php/load",
            ],
        )
        cn_requests = [
            record
            for record in server.requests
            if record["path"].startswith("/api/index.php/")
        ]
        self.assertTrue(
            all(
                record["content_type"] == "application/x-www-form-urlencoded"
                for record in cn_requests
            )
        )
        guest_request = next(
            record
            for record in server.requests
            if record["path"] == "/mobile!guestRegister.action"
        )
        self.assertEqual(
            guest_request["content_type"],
            "application/x-www-form-urlencoded",
        )
        leiting_request = next(
            record
            for record in cn_requests
            if record["path"]
            == "/api/index.php/channels/channel_leiting/leiting_login"
        )
        self.assertEqual(leiting_request["decoded"], {"userId": "10000001"})
        signup_request = next(
            record
            for record in cn_requests
            if record["path"] == "/api/index.php/tool/signup"
        )
        self.assertEqual(signup_request["decoded"], {"device_id": 57})
        load_requests = [
            record
            for record in cn_requests
            if record["path"] == "/api/index.php/load"
        ]
        self.assertEqual(
            load_requests[0]["decoded"],
            {"keychain": VIEWER_ID, "viewer_id": VIEWER_ID},
        )
        self.assertGreaterEqual(len(load_requests), 3)
        tutorial_request = next(
            record
            for record in cn_requests
            if record["path"] == "/api/index.php/tutorial/update_step"
        )
        self.assertEqual(
            tutorial_request["decoded"],
            {"viewer_id": VIEWER_ID, "step": 0, "skip": True},
        )
        asset_requests = [
            record
            for record in cn_requests
            if record["path"].startswith("/api/index.php/asset")
        ]
        self.assertEqual(len(asset_requests), 3)
        self.assertTrue(
            all(record["headers"].get("device") == "1" for record in asset_requests)
        )
        self.assertTrue(
            all(record["headers"].get("res_ver") == "1.4.8" for record in asset_requests)
        )
        battle_start = next(
            record
            for record in cn_requests
            if record["path"] == "/api/index.php/single_battle_quest/start"
        )
        self.assertRegex(
            battle_start["decoded"]["play_id"],
            r"^ios-cn-scenario-\d+$",
        )
        self.assertEqual(
            [
                record["decoded"]["type"]
                for record in cn_requests
                if record["path"] == "/api/index.php/gacha/exec"
            ],
            [1, 2, 1],
        )

        serialized = json.dumps(report)
        self.assertNotIn(str(VIEWER_ID), serialized)
        self.assertNotIn(LOGIN_TOKEN, serialized)
        self.assertIsNone(
            re.search(
                r"viewer_id|login_token|keychain|mail_id|player_snapshot|Authorization|Bearer",
                serialized,
            )
        )

    def test_failure_blocks_following_stages(self):
        failed_path = "/api/index.php/single_battle_quest/start"
        with _ScenarioServer(fail_path=failed_path) as server:
            report = RUNNER.run_cn_game_scenarios(
                server.base_url,
                device_id=58,
                sleep_impl=lambda milliseconds: None,
            )

        self.assertEqual(report["status"], "failed")
        self.assertEqual(
            report["first_failure"],
            {"stage": "single_battle_start", "error_code": "CN_HTTP_404"},
        )
        self.assertEqual(report["last_successful_stage"], "virtual_time")
        failed_index = RUNNER.STAGE_NAMES.index("single_battle_start")
        self.assertEqual(report["stages"][failed_index]["status"], "failed")
        self.assertTrue(
            all(
                stage["status"] == "blocked"
                for stage in report["stages"][failed_index + 1 :]
            )
        )
        self.assertEqual(server.requests[-1]["path"], failed_path)

    def test_completed_tutorial_does_not_repeat_update_step(self):
        with _ScenarioServer() as server:
            server.http_server.state["user_tutorial"] = None
            report = RUNNER.run_cn_game_scenarios(
                server.base_url,
                device_id=61,
                sleep_impl=lambda milliseconds: None,
            )

        self.assertEqual(report["status"], "passed")
        tutorial_stage = next(
            stage for stage in report["stages"] if stage["name"] == "tutorial_skip"
        )
        self.assertEqual(
            tutorial_stage["evidence"],
            {
                "skip_applied": False,
                "already_completed": True,
                "result_step": None,
            },
        )
        self.assertFalse(
            any(
                request["path"] == "/api/index.php/tutorial/update_step"
                for request in server.requests
            )
        )

    def test_asset_versions_accept_supported_entity_list_directories(self):
        for directory in ("entities", "EntityLists"):
            with self.subTest(directory=directory):
                with _ScenarioServer(entity_list_directory=directory) as server:
                    report = RUNNER.run_cn_game_scenarios(
                        server.base_url,
                        device_id=62,
                        sleep_impl=lambda milliseconds: None,
                    )

                self.assertEqual(report["status"], "passed")
                self.assertEqual(
                    [
                        request["path"]
                        for request in server.requests
                        if request["method"] == "GET" and request["path"].endswith(".csv")
                    ],
                    [
                        "/patch/cn/%s/%s" % (directory, IOS_TITLE_ENTITY_LIST_NAME),
                        "/patch/cn/%s/%s" % (directory, GAME_ENTITY_LIST_NAME),
                    ],
                )

    def test_asset_versions_reject_different_entity_list_sources(self):
        with _ScenarioServer(
            entity_list_directory="entities",
            game_entity_list_directory="EntityLists",
        ) as server:
            report = RUNNER.run_cn_game_scenarios(
                server.base_url,
                device_id=63,
                sleep_impl=lambda milliseconds: None,
            )

        self.assertEqual(
            report["first_failure"],
            {"stage": "asset_version", "error_code": "ASSET_VERSION_MISMATCH"},
        )

    def test_response_urls_remain_loopback_and_redacted(self):
        for response_url_stage, expected_stage in (
            ("health", "health"),
            ("signup", "signup"),
        ):
            with self.subTest(response_url_stage=response_url_stage):
                with _ScenarioServer(response_url_stage=response_url_stage) as server:
                    report = RUNNER.run_cn_game_scenarios(
                        server.base_url,
                        device_id=59,
                        sleep_impl=lambda milliseconds: None,
                    )
                self.assertEqual(
                    report["first_failure"],
                    {
                        "stage": expected_stage,
                        "error_code": "NON_LOOPBACK_RESPONSE_URL",
                    },
                )
                serialized = json.dumps(report)
                self.assertNotIn(EXTERNAL_URL, serialized)
                self.assertNotIn("must-not-leak.invalid", serialized)
                self.assertNotIn("response-secret", serialized)

    def test_archive_redirect_remains_loopback_and_redacted(self):
        with _ScenarioServer(redirect_archive_external=True) as server:
            report = RUNNER.run_cn_game_scenarios(
                server.base_url,
                device_id=60,
                sleep_impl=lambda milliseconds: None,
            )
        self.assertEqual(
            report["first_failure"],
            {
                "stage": "asset_archive_download",
                "error_code": "NON_LOOPBACK_RESPONSE_URL",
            },
        )
        serialized = json.dumps(report)
        self.assertNotIn(EXTERNAL_URL, serialized)
        self.assertNotIn("must-not-leak.invalid", serialized)
        self.assertNotIn("response-secret", serialized)

    def test_non_loopback_base_url_is_rejected(self):
        with self.assertRaises(RUNNER.ScenarioFailure) as raised:
            RUNNER.run_cn_game_scenarios("https://example.com")
        self.assertEqual(raised.exception.code, "NON_LOOPBACK_BASE_URL")
        self.assertEqual(
            RUNNER.normalized_base_url("http://[::1]:17171/path?query=1"),
            "http://[::1]:17171",
        )


if __name__ == "__main__":
    unittest.main()
