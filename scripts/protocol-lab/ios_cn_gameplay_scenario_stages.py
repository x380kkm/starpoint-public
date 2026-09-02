# audience: internal
# # ios-cn-gameplay-scenario-stages
# 该模块执行 iOS Simulator CN 协议链中的时间, 战斗, 扭蛋, 邮件和活动阶段.

import json
import math
from pathlib import Path
from urllib.parse import quote

from ios_cn_game_scenario_stages import (
    MULTI_DRAW_COST,
    SINGLE_DRAW_COST,
    StartupScenarioStages,
    _gacha_info,
    _nested,
    _number,
    _numeric_value,
    require_scenario,
)

MIN_SINGLE_BATTLE_ITEM_DROP = 1
SCENARIO_GACHA_ID = 80000
SCENARIO_EQUIPMENT_GACHA_ID = 3


def _gacha_candidate_ids(gacha_id):
    gacha = _gacha_definition(gacha_id)
    pool = gacha.get("pool", {})
    return {
        int(item["id"])
        for entries in pool.values()
        for item in entries
        if isinstance(item, dict) and isinstance(item.get("id"), int)
    }


def _gacha_definition(gacha_id):
    asset_path = Path(__file__).resolve().parents[2] / "assets" / "gacha.json"
    try:
        document = json.loads(asset_path.read_text(encoding="utf-8"))
        gacha = document.get(str(gacha_id), {})
        return gacha if isinstance(gacha, dict) else {}
    except (OSError, ValueError, TypeError, KeyError):
        return {}


def _open_drawable_pool(pools, gacha_type):
    candidates = []
    for entry in pools:
        activity_id = _nested(entry, "activity_id")
        if (
            not isinstance(entry, dict)
            or _nested(entry, "status") != "open"
            or not isinstance(activity_id, str)
            or not activity_id.startswith("gacha:")
        ):
            continue
        try:
            gacha_id = int(activity_id.split(":", 1)[1])
        except ValueError:
            continue
        definition = _gacha_definition(gacha_id)
        if (
            definition.get("type") != gacha_type
            or definition.get("pageKind") not in (0, 8)
            or not isinstance(definition.get("singleCost"), int)
            or definition["singleCost"] <= 0
        ):
            continue
        candidates.append((definition.get("pageKind") != 0, gacha_id, entry))
    return min(candidates, default=(None, None, None))[2]


# //// 执行 CN 游戏玩法场景阶段 [@x380kkm 2026-08-21] ////
class ScenarioStages(StartupScenarioStages):
    def virtual_time(self):
        first = self.request_json("GET", "/v1/time")
        first_time = _numeric_value(
            _nested(first, "unix_time_ms"),
            "VIRTUAL_TIME_INVALID",
            "virtual time did not return unix_time_ms",
        )
        require_scenario(
            _nested(first, "enabled") is True,
            "VIRTUAL_TIME_DISABLED",
            "a pristine Simulator service did not enable virtual time",
        )
        require_scenario(
            _number(_nested(first, "rate")) == 1,
            "VIRTUAL_TIME_RATE_INVALID",
            "the default virtual time rate is not 1",
        )
        self.sleep_impl(25)
        second = self.request_json("GET", "/v1/time")
        second_time = _numeric_value(
            _nested(second, "unix_time_ms"),
            "VIRTUAL_TIME_INVALID",
            "advanced virtual time did not return unix_time_ms",
        )
        require_scenario(
            second_time > first_time,
            "VIRTUAL_TIME_NOT_ADVANCING",
            "virtual time did not advance with system time",
        )
        return {"enabled": True, "rate": 1, "advances_with_system_time": True}

    def single_battle_start(self):
        response = self.request_cn(
            "/api/index.php/single_battle_quest/start",
            {
                "viewer_id": self.viewer_id,
                "api_count": 1,
                "quest_id": 1001002,
                "use_boss_boost_point": False,
                "use_boost_point": False,
                "category": 1,
                "play_id": self.play_id,
                "is_auto_start_mode": False,
                "party_id": 1,
            },
        )
        require_scenario(
            _number(_nested(response, "data", "category_id")) == 1,
            "BATTLE_START_INVALID",
            "single battle did not start in category 1",
        )
        require_scenario(
            _number(_nested(response, "data", "user_info", "last_main_quest_id"))
            == 1001002,
            "BATTLE_QUEST_INVALID",
            "single battle did not bind the expected verified quest",
        )
        return {"quest_started": True, "quest_id": 1001002, "category": 1}

    def single_battle_finish(self):
        response = self.request_cn(
            "/api/index.php/single_battle_quest/finish",
            {
                "viewer_id": self.viewer_id,
                "api_count": 1,
                "is_restored": False,
                "continue_count": 0,
                "elapsed_time_ms": 100000,
                "quest_id": 1001002,
                "play_id": self.play_id,
                "category": 1,
                "score": 1000,
                "add_mana": 7,
                "is_accomplished": True,
                "statistics": {
                    "clear_phase": 1,
                    "party": {
                        "characters": [{"id": 1}, None, None],
                        "unison_characters": [None, None, None],
                        "equipments": [None, None, None],
                        "ability_soul_ids": [None, None, None],
                    },
                },
            },
        )
        require_scenario(
            _number(_nested(response, "data", "clear_rank")) == 5,
            "BATTLE_FINISH_INVALID",
            "single battle did not return the verified clear rank",
        )
        require_scenario(
            _number(_nested(response, "data", "rewards", "reward_mana")) == 20,
            "BATTLE_REWARD_INVALID",
            "single battle did not return the verified reward",
        )
        require_scenario(
            _number(_nested(response, "data", "item_list", "13"))
            >= MIN_SINGLE_BATTLE_ITEM_DROP,
            "BATTLE_DROP_INVALID",
            "single battle did not return the verified item drop",
        )
        self.battle_data = _nested(response, "data")
        return {
            "quest_finished": True,
            "reward_applied": True,
            "item_drop_applied": True,
        }

    def gacha_resource_mail_create(self):
        created = self.request_json(
            "POST",
            "/v1/mails",
            {
                "viewer_id": self.viewer_id,
                "title": "Gacha scenario resources",
                "body": "iOS Simulator scenario prerequisite",
                "sender": "Starpoint",
                "rewards": {"freeVmoney": 1725},
            },
        )
        mail_id = _number(_nested(created, "id"))
        require_scenario(
            isinstance(mail_id, int) and mail_id > 0,
            "GACHA_RESOURCE_MAIL_INVALID",
            "gacha prerequisite mail was not created",
        )
        return {
            "prerequisite_created": True,
            "resource_kind": "free_vmoney",
            "amount": 1725,
        }

    def gacha_resource_mail_receive(self):
        self.request_cn(
            "/api/index.php/mail/receive_all",
            {"viewer_id": self.viewer_id},
        )
        response = self.request_cn(
            "/api/index.php/load",
            {"keychain": self.viewer_id, "viewer_id": self.viewer_id},
        )
        free_vmoney = _numeric_value(
            _nested(response, "data", "user_info", "free_vmoney"),
            "GACHA_RESOURCE_MISSING",
            "gacha prerequisite mail did not grant free currency",
        )
        require_scenario(
            free_vmoney >= SINGLE_DRAW_COST + MULTI_DRAW_COST,
            "GACHA_RESOURCE_INSUFFICIENT",
            "gacha prerequisite resources are insufficient",
        )
        self.gacha_currency_before = free_vmoney
        self.mail_mana_before = _numeric_value(
            _nested(response, "data", "user_info", "free_mana"),
            "MAIL_BASELINE_MISSING",
            "mail receipt did not expose the mana baseline",
        )
        return {
            "prerequisite_received": True,
            "sufficient_for_single_and_ten": True,
        }

    def gacha_single(self):
        response = self.request_cn(
            "/api/index.php/gacha/exec",
            {
                "api_count": 1,
                "payment_type": 1,
                "number_of_exec": 1,
                "viewer_id": self.viewer_id,
                "gacha_id": self.character_gacha_id,
                "type": 1,
            },
        )
        draws = _nested(response, "data", "draw")
        require_scenario(
            isinstance(draws, list) and len(draws) == 1,
            "GACHA_SINGLE_DRAW_INVALID",
            "single gacha did not return one draw",
        )
        after = _numeric_value(
            _nested(response, "data", "user_info", "free_vmoney"),
            "GACHA_CURRENCY_MISSING",
            "single gacha did not return currency state",
        )
        require_scenario(
            self.gacha_currency_before - after == SINGLE_DRAW_COST,
            "GACHA_SINGLE_COST_INVALID",
            "single gacha did not deduct the verified cost",
        )
        require_scenario(
            isinstance(_gacha_info(_nested(response, "data"), self.character_gacha_id), dict),
            "GACHA_EXEC_POOL_MISMATCH",
            "single gacha did not return the selected pool state",
        )
        character_id = _numeric_value(
            _nested(draws[0], "character_id"),
            "GACHA_CHARACTER_MISSING",
            "single gacha did not return a character",
        )
        if self.gacha_candidates:
            require_scenario(
                character_id in self.gacha_candidates,
                "GACHA_RESULT_OUTSIDE_POOL",
                "single gacha returned a character outside the active candidate pool",
            )
        self.gacha_draw_character_ids.append(character_id)
        self.gacha_currency_after_single = after
        return {
            "draw_count": 1,
            "cost": SINGLE_DRAW_COST,
            "character_or_duplicate_applied": True,
        }

    def gacha_ten(self):
        response = self.request_cn(
            "/api/index.php/gacha/exec",
            {
                "api_count": 1,
                "payment_type": 1,
                "number_of_exec": 1,
                "viewer_id": self.viewer_id,
                "gacha_id": self.character_gacha_id,
                "type": 2,
            },
        )
        draws = _nested(response, "data", "draw")
        require_scenario(
            isinstance(draws, list) and len(draws) == 10,
            "GACHA_TEN_DRAW_INVALID",
            "ten-pull gacha did not return ten draws",
        )
        after = _numeric_value(
            _nested(response, "data", "user_info", "free_vmoney"),
            "GACHA_CURRENCY_MISSING",
            "ten-pull gacha did not return currency state",
        )
        expected = self.gacha_currency_after_single - MULTI_DRAW_COST
        require_scenario(
            after == expected,
            "GACHA_TEN_COST_INVALID",
            "ten-pull gacha did not deduct the verified cost",
        )
        info = _gacha_info(_nested(response, "data"), self.character_gacha_id)
        require_scenario(
            _number(_nested(info, "gacha_exchange_point")) >= 11,
            "GACHA_POINT_INVALID",
            "gacha exchange points did not include eleven draws",
        )
        item_list = _nested(response, "data", "item_list")
        require_scenario(
            isinstance(item_list, dict),
            "GACHA_ITEM_LIST_INVALID",
            "character gacha did not return its item list",
        )
        duplicate_item_ids = set()
        for draw in draws:
            character_id = _numeric_value(
                _nested(draw, "character_id"),
                "GACHA_CHARACTER_MISSING",
                "ten-pull gacha returned a draw without a character",
            )
            if self.gacha_candidates:
                require_scenario(
                    character_id in self.gacha_candidates,
                    "GACHA_RESULT_OUTSIDE_POOL",
                    "ten-pull gacha returned a character outside the active candidate pool",
                )
            duplicate_item = _nested(draw, "ex_boost_item")
            if isinstance(duplicate_item, dict):
                duplicate_item_id = _numeric_value(
                    _nested(duplicate_item, "id"),
                    "GACHA_DUPLICATE_REWARD_INVALID",
                    "duplicate character conversion returned an invalid item id",
                )
                require_scenario(
                    _number(_nested(duplicate_item, "count")) >= 1,
                    "GACHA_DUPLICATE_REWARD_INVALID",
                    "duplicate character conversion returned an invalid item count",
                )
                duplicate_item_ids.add(int(duplicate_item_id))
            self.gacha_draw_character_ids.append(character_id)
        for duplicate_item_id in duplicate_item_ids:
            require_scenario(
                _number(_nested(item_list, str(duplicate_item_id))) >= 1,
                "GACHA_DUPLICATE_REWARD_INVALID",
                "duplicate character conversion was not included in item_list",
            )
        self.gacha_duplicate_item_ids = duplicate_item_ids
        self.final_expected_free_vmoney = expected
        return {
            "draw_count": 10,
            "cost": MULTI_DRAW_COST,
            "character_or_duplicate_applied": True,
        }

    def gacha_pool_consistency(self):
        catalog = self.request_json("GET", "/v1/activities/catalog?kind=gacha")
        pools = _nested(catalog, "activities")
        require_scenario(
            isinstance(pools, list),
            "GACHA_CATALOG_INVALID",
            "the activity catalog did not return gacha entries",
        )
        pool = _open_drawable_pool(pools, 0)
        require_scenario(
            isinstance(pool, dict),
            "GACHA_POOL_MISSING",
            "the activity catalog did not expose the requested gacha pool",
        )
        require_scenario(
            _nested(pool, "status") == "open",
            "GACHA_POOL_CLOSED",
            "the requested gacha pool is not open at virtual time",
        )
        require_scenario(
            isinstance(_nested(pool, "banner_key"), str)
            and bool(_nested(pool, "banner_key"))
            and _number(_nested(pool, "banner_width")) == 510
            and _number(_nested(pool, "banner_height")) == 180,
            "GACHA_BANNER_MISSING",
            "the open gacha pool has no banner key",
        )
        activity_id = _nested(pool, "activity_id")
        self.character_gacha_id = int(activity_id.split(":", 1)[1])
        load = self.request_cn(
            "/api/index.php/load",
            {"keychain": self.viewer_id, "viewer_id": self.viewer_id},
        )
        info = _gacha_info(_nested(load, "data"), self.character_gacha_id)
        require_scenario(
            not isinstance(info, dict)
            or _number(_nested(info, "gacha_id")) == self.character_gacha_id,
            "GACHA_LOAD_POOL_MISMATCH",
            "load advertised a conflicting gacha pool state",
        )
        self.gacha_candidates = _gacha_candidate_ids(self.character_gacha_id)
        equipment_pool = _open_drawable_pool(pools, 1)
        if isinstance(equipment_pool, dict):
            activity_id = _nested(equipment_pool, "activity_id")
            if isinstance(activity_id, str) and activity_id.startswith("gacha:"):
                try:
                    self.equipment_gacha_id = int(activity_id.split(":", 1)[1])
                except ValueError:
                    self.equipment_gacha_id = SCENARIO_EQUIPMENT_GACHA_ID
            else:
                self.equipment_gacha_id = SCENARIO_EQUIPMENT_GACHA_ID
        else:
            self.equipment_gacha_id = SCENARIO_EQUIPMENT_GACHA_ID
        return {
            "gacha_id": self.character_gacha_id,
            "open_at_virtual_time": True,
            "banner_present": True,
            "candidate_source": "cn_gacha_master",
        }

    def equipment_gacha(self):
        require_scenario(
            isinstance(self.equipment_gacha_id, int) and self.equipment_gacha_id > 0,
            "EQUIPMENT_POOL_MISSING",
            "the activity catalog did not expose an equipment gacha pool",
        )
        response = self.request_cn(
            "/api/index.php/gacha/exec",
            {
                "api_count": 1,
                "payment_type": 1,
                "number_of_exec": 1,
                "viewer_id": self.viewer_id,
                "gacha_id": self.equipment_gacha_id,
                "type": 1,
            },
        )
        equipment_draws = _nested(response, "data", "draw_equipment")
        require_scenario(
            isinstance(equipment_draws, list) and len(equipment_draws) == 1,
            "EQUIPMENT_GACHA_DRAW_INVALID",
            "equipment gacha did not return one equipment draw",
        )
        equipment_list = _nested(response, "data", "equipment_list")
        require_scenario(
            isinstance(equipment_list, list) and len(equipment_list) > 0,
            "EQUIPMENT_GACHA_LIST_INVALID",
            "equipment gacha did not return equipment state",
        )
        item_list = _nested(response, "data", "item_list")
        require_scenario(
            isinstance(item_list, dict),
            "GACHA_ITEM_LIST_INVALID",
            "equipment gacha did not return its item list",
        )
        require_scenario(
            isinstance(_gacha_info(_nested(response, "data"), self.equipment_gacha_id), dict),
            "EQUIPMENT_GACHA_POOL_MISMATCH",
            "equipment gacha did not return the selected pool state",
        )
        self.equipment_ids = [
            _numeric_value(
                _nested(entry, "equipment_id"),
                "EQUIPMENT_ID_INVALID",
                "equipment gacha returned an invalid equipment identifier",
            )
            for entry in equipment_draws
        ]
        self.final_expected_free_vmoney = _numeric_value(
            _nested(response, "data", "user_info", "free_vmoney"),
            "GACHA_CURRENCY_MISSING",
            "equipment gacha did not return currency state",
        )
        return {
            "gacha_id": self.equipment_gacha_id,
            "draw_count": len(equipment_draws),
            "equipment_state_returned": True,
            "item_state_returned": True,
        }

    def multiplayer_ai_handshake(self):
        require_scenario(
            callable(self.request_session),
            "MULTI_SESSION_UNAVAILABLE",
            "the multiplayer loopback session transport is unavailable",
        )
        created = self.request_cn(
            "/api/index.php/multi_battle_quest/create_room",
            {
                "category": 1,
                "party_id": 1,
                "quest_id": 1001002,
                "viewer_id": self.viewer_id,
                "api_count": 1,
            },
        )
        room = _nested(created, "data")
        room_number = _nested(room, "room_number")
        require_scenario(
            isinstance(room_number, str) and room_number,
            "MULTI_ROOM_CREATE_INVALID",
            "multiplayer room creation did not return a room number",
        )
        selected = self.request_cn(
            "/api/index.php/multi_battle_quest/select_room",
            {
                "category": 1,
                "quest_id": 1001002,
                "party_id": 1,
                "accepted_type": 0,
                "viewer_id": self.viewer_id,
                "room_number": room_number,
                "api_count": 2,
            },
        )
        selected_data = _nested(selected, "data")
        require_scenario(
            _number(_nested(selected_data, "raising_state")) in (1, 2),
            "MULTI_ROOM_SELECT_INVALID",
            "multiplayer room selection returned an invalid state",
        )
        session_port = _number(_nested(selected_data, "port"))
        require_scenario(
            isinstance(session_port, (int, float)) and session_port > 0,
            "MULTI_SESSION_PORT_INVALID",
            "multiplayer room selection did not return a session port",
        )
        summoned = self.request_cn(
            "/api/index.php/multi_battle_quest/summon",
            {
                "category_id": 1,
                "quest_id": 1001002,
                "room_number": room_number,
                "viewer_id": self.viewer_id,
                "api_count": 3,
            },
        )
        mate1 = _nested(summoned, "data", "mate1")
        mate2 = _nested(summoned, "data", "mate2")
        require_scenario(
            isinstance(mate1, dict)
            and isinstance(_nested(mate1, "party"), dict)
            and isinstance(mate2, dict)
            and isinstance(_nested(mate2, "party"), dict),
            "MULTI_AI_SUMMON_INVALID",
            "multiplayer summon did not return two AI parties",
        )
        connection_frame = self.request_session(
            int(session_port),
            {
                "socklet": "cooperation_room",
                "viewerId": self.viewer_id,
                "roomNumber": room_number,
                "questCategory": 1,
                "questId": 1001002,
                "reconnected": 0,
            },
        )
        require_scenario(
            isinstance(connection_frame, list)
            and len(connection_frame) >= 3
            and connection_frame[0] == 0,
            "MULTI_SESSION_HANDSHAKE_INVALID",
            "multiplayer session handshake did not return a connection frame",
        )
        connection_id = connection_frame[1]
        party = {
            "characters": [
                [0, {"id": 1, "evolution_level": 0, "exp": 10, "over_limit_step": 0, "mana_node_ids": {}, "illustration_settings": [1], "ex_boost": [1]}],
                [1],
                [1],
            ],
            "unison_characters": [[1], [1], [1]],
            "equipments": [[1], [1], [1]],
            "abilitySoulIds": [[1], [1], [1]],
            "options": None,
        }
        welcome = self.request_session(
            int(session_port),
            [0, [0, {"viewerId": self.viewer_id, "playerId": 999, "name": "Host", "rank": 1, "degreeId": 1, "mainCharacterId": 999, "party": party, "connectionId": connection_id, "playerRoleKind": 1, "isNewbie": False, "isHost": True, "entryTime": 0, "currentPartyId": 1, "autoplayMode": False, "autoskillMode": 1, "autoSpeedLevel": 1, "autoStart": False, "skillAbilityBehaviorMode": 1, "dashBehaviorMode": 1, "allowHealFromOtherPlayers": True, "state": [0]}, 1]],
        )
        require_scenario(
            isinstance(welcome, list) and len(welcome) > 1 and welcome[1][0] == 0,
            "MULTI_SESSION_ENTER_INVALID",
            "multiplayer session did not accept the configured host party",
        )
        self.request_session(int(session_port), [0, [99]], receive_count=0)
        heartbeat = self.request_session(int(session_port), [0, [4]])
        require_scenario(
            isinstance(heartbeat, list) and heartbeat[1][0] == 11,
            "MULTI_HEARTBEAT_INVALID",
            "multiplayer legacy heartbeat did not return its connection frame",
        )
        ai_requests = [
            {
                "degreeId": _nested(mate1, "degree_id"),
                "rank": _nested(mate1, "rank"),
                "name": "COM Mate 1",
                "comId": _nested(mate1, "com_id"),
                "party": _nested(mate1, "party"),
            },
            {
                "degreeId": _nested(mate2, "degree_id"),
                "rank": _nested(mate2, "rank"),
                "name": "COM Mate 2",
                "comId": _nested(mate2, "com_id"),
                "party": _nested(mate2, "party"),
            },
        ]
        sequence = self.request_session(
            int(session_port),
            [0, [10, ai_requests]],
            receive_count=5,
        )
        require_scenario(
            isinstance(sequence, list)
            and len(sequence) == 5
            and sequence[0][1][0] == 1
            and sequence[1][1][0] == 2
            and sequence[2][1][0] == 2
            and sequence[3][1][0] == 2
            and sequence[4][1][0] == 10,
            "MULTI_AI_SEQUENCE_INVALID",
            "multiplayer AI join sequence did not produce a lobby frame",
        )
        readiness = self.request_session(int(session_port), [0, [6]])
        require_scenario(
            isinstance(readiness, list) and readiness[1][0] == 5,
            "MULTI_READY_FRAME_INVALID",
            "multiplayer ready handshake did not produce the battle-start frame",
        )
        started = self.request_cn(
            "/api/index.php/multi_battle_quest/start",
            {
                "quest_id": 1001002,
                "use_boss_boost_point": False,
                "use_boost_point": False,
                "category": 1,
                "viewer_id": self.viewer_id,
                "play_id": self.play_id + "-multi",
                "is_auto_start_mode": False,
                "party_id": 1,
                "room_number": room_number,
                "mate_player_ids": [],
                "mate_party_ids": [],
                "api_count": 4,
            },
        )
        started_data = _nested(started, "data")
        require_scenario(
            _nested(started_data, "is_multi") == "multi",
            "MULTI_START_INVALID",
            "multiplayer start did not return the multi battle shape",
        )
        battle_handshake = self.request_session(
            int(session_port),
            {
                "socklet": "cooperation_battle",
                "roomNumber": room_number,
                "connectionId": connection_id,
                "reconnected": 0,
            },
            channel="battle",
        )
        require_scenario(
            isinstance(battle_handshake, list)
            and len(battle_handshake) == 3
            and battle_handshake[0] == 0
            and battle_handshake[1] == room_number,
            "MULTI_BATTLE_HANDSHAKE_INVALID",
            "multiplayer battle socket did not accept the lobby connection",
        )
        scene_ready = self.request_session(
            int(session_port), [0, [0]], channel="battle"
        )
        require_scenario(
            scene_ready == [1, [1]],
            "MULTI_BATTLE_READY_INVALID",
            "multiplayer battle socket did not acknowledge scene readiness",
        )
        battle_finish = self.request_session(
            int(session_port), [0, [2]], channel="battle"
        )
        require_scenario(
            battle_finish == [1, [2]],
            "MULTI_BATTLE_FINISH_INVALID",
            "multiplayer battle socket did not acknowledge battle completion",
        )
        finished = self.request_cn(
            "/api/index.php/multi_battle_quest/finish",
            {
                "viewer_id": self.viewer_id,
                "quest_id": 1001002,
                "category": 1,
                "clear_phase": 1,
                "quest_statistics": {"party": {"characters": [], "unison_characters": [], "equipments": [], "ability_soul_ids": []}},
                "play_id": self.play_id + "-multi",
                "battle_time": 1,
                "battle_ended_at": 1,
                "api_count": 5,
                "mate_player_ids": [],
                "mate_com_ids": [],
                "is_auto_start_mode": False,
                "combat_power": 1,
                "use_boss_boost_point": False,
                "use_boost_point": False,
                "is_accomplished": True,
            },
        )
        require_scenario(
            _nested(finished, "data", "is_multi") == "multi",
            "MULTI_FINISH_INVALID",
            "multiplayer finish did not return the multi result",
        )
        self.mail_mana_before = _numeric_value(
            _nested(finished, "data", "user_info", "free_mana"),
            "MULTI_REWARD_STATE_MISSING",
            "multiplayer finish did not return the current mana state",
        )
        return {
            "room_created": True,
            "ai_party_returned": True,
            "lobby_handshake_completed": True,
            "battle_handshake_completed": True,
            "multi_start_acknowledged": True,
            "multi_finish_acknowledged": True,
        }

    def mail_create(self):
        created = self.request_json(
            "POST",
            "/v1/mails",
            {
                "viewer_id": self.viewer_id,
                "title": "Mail scenario reward",
                "body": "iOS Simulator mail lifecycle",
                "sender": "Starpoint",
                "rewards": {"freeMana": 50},
            },
        )
        mail_id = _number(_nested(created, "id"))
        require_scenario(
            isinstance(mail_id, int) and mail_id > 0,
            "MAIL_CREATE_INVALID",
            "management mail was not created",
        )
        self.mail_id = int(mail_id)
        return {
            "mail_created": True,
            "reward_kind": "free_mana",
            "reward_amount": 50,
        }

    def mail_list(self):
        response = self.request_cn(
            "/api/index.php/mail/index",
            {"viewer_id": self.viewer_id, "current_page": 1},
        )
        mails = _nested(response, "data", "mail")
        visible = isinstance(mails, list) and any(
            _number(_nested(mail, "id")) == self.mail_id for mail in mails
        )
        require_scenario(
            visible,
            "MAIL_LIST_MISSING",
            "the game mail list did not contain the management mail",
        )
        return {"mail_visible_in_game": True}

    def mail_receive(self):
        response = self.request_cn(
            "/api/index.php/mail/receive",
            {"viewer_id": self.viewer_id, "mail_id": self.mail_id},
        )
        after = _numeric_value(
            _nested(response, "data", "user_info", "free_mana"),
            "MAIL_REWARD_MISSING",
            "mail receive did not return the reward state",
        )
        require_scenario(
            after == self.mail_mana_before + 50,
            "MAIL_REWARD_INVALID",
            "mail receive did not grant the configured reward",
        )
        return {"mail_received": True, "reward_applied": True}

    def activity_catalog(self):
        catalog = self.request_json("GET", "/v1/activities/catalog")
        require_scenario(
            _nested(catalog, "manifest_state") == "loaded",
            "ACTIVITY_CATALOG_UNAVAILABLE",
            "the activity catalog manifest is not loaded",
        )
        activities = _nested(catalog, "activities")
        require_scenario(
            isinstance(activities, list) and len(activities) > 0,
            "ACTIVITY_CATALOG_EMPTY",
            "the activity catalog has no activities",
        )
        selected = None
        for activity in activities:
            activity_id = _nested(activity, "activity_id")
            if isinstance(activity_id, str) and activity_id:
                selected = activity_id
                break
        require_scenario(
            selected is not None,
            "ACTIVITY_ID_MISSING",
            "the activity catalog has no valid activity identifier",
        )
        self.activity_id = selected
        return {
            "manifest_loaded": True,
            "activity_count": len(activities),
            "activity_selected": True,
        }

    def activity_close(self):
        encoded = quote(self.activity_id, safe="")
        response = self.request_json(
            "POST",
            "/v1/activities/%s/close" % encoded,
            {},
        )
        require_scenario(
            _nested(response, "status") == "disabled",
            "ACTIVITY_CLOSE_INVALID",
            "the selected activity did not close",
        )
        return {"activity_closed": True}

    def activity_open(self):
        encoded = quote(self.activity_id, safe="")
        response = self.request_json(
            "POST",
            "/v1/activities/%s/open" % encoded,
            {},
        )
        require_scenario(
            _nested(response, "status") == "open",
            "ACTIVITY_OPEN_INVALID",
            "the selected activity did not open",
        )
        return {"activity_opened": True}

    def checkpoint(self):
        response = self.request_json("POST", "/v1/checkpoint")
        require_scenario(
            _nested(response, "status") == "ok",
            "CHECKPOINT_INVALID",
            "the personal service checkpoint failed",
        )
        return {"checkpoint_completed": True}

    def load_persistence(self):
        response = self.request_cn(
            "/api/index.php/load",
            {"keychain": self.viewer_id, "viewer_id": self.viewer_id},
        )
        data = _nested(response, "data")
        require_scenario(
            _number(_nested(data, "user_info", "free_vmoney"))
            == self.final_expected_free_vmoney,
            "GACHA_PERSISTENCE_INVALID",
            "gacha currency did not persist through checkpoint",
        )
        require_scenario(
            _number(_nested(data, "user_info", "free_mana"))
            == self.mail_mana_before + 50,
            "MAIL_PERSISTENCE_INVALID",
            "mail reward did not persist through checkpoint",
        )
        rank_point = _number(_nested(data, "user_info", "rank_point"))
        require_scenario(
            math.isfinite(rank_point) and rank_point >= 0,
            "BATTLE_PERSISTENCE_INVALID",
            "battle rank state did not persist through checkpoint",
        )
        require_scenario(
            _number(_nested(data, "item_list", "13")) >= MIN_SINGLE_BATTLE_ITEM_DROP,
            "BATTLE_DROP_PERSISTENCE_INVALID",
            "battle item drop did not persist through checkpoint",
        )
        characters = _nested(data, "user_character_list")
        if not isinstance(characters, dict):
            characters = {}
        persisted = all(
            str(character_id) in characters
            for character_id in self.gacha_draw_character_ids
        )
        require_scenario(
            persisted,
            "GACHA_CHARACTER_PERSISTENCE_INVALID",
            "one or more gacha characters did not persist through checkpoint",
        )
        require_scenario(
            _number(
                _nested(
                    _gacha_info(data, self.character_gacha_id),
                    "gacha_exchange_point",
                )
            )
            >= 11,
            "GACHA_POINT_PERSISTENCE_INVALID",
            "gacha exchange points did not persist through checkpoint",
        )
        exp_pool = _nested(data, "user_info", "exp_pool")
        require_scenario(
            _number(exp_pool) >= 0,
            "EXP_POOL_PERSISTENCE_INVALID",
            "the experience pool did not persist through checkpoint",
        )
        item_list = _nested(data, "item_list")
        require_scenario(
            isinstance(item_list, dict),
            "GACHA_REWARD_PERSISTENCE_INVALID",
            "gacha reward materials did not persist through checkpoint",
        )
        for duplicate_item_id in getattr(self, "gacha_duplicate_item_ids", set()):
            require_scenario(
                _number(_nested(item_list, str(duplicate_item_id))) >= 1,
                "GACHA_REWARD_PERSISTENCE_INVALID",
                "duplicate character material did not persist through checkpoint",
            )
        return {
            "battle_persisted": True,
            "gacha_persisted": True,
            "mail_persisted": True,
            "exp_pool_persisted": True,
            "gacha_rewards_persisted": True,
            "character_count": len(characters),
        }
