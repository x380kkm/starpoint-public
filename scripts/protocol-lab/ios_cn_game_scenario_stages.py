# audience: internal
# # ios-cn-game-scenario-stages
# 该模块保存 iOS Simulator CN 协议链的阶段顺序, 状态和逐阶段断言.

import base64
import binascii
import math
import os
import time


SINGLE_DRAW_COST = 150
MULTI_DRAW_COST = 1500
GACHA_RESOURCE_GRANT = 1650
MAX_SAFE_INTEGER = (1 << 53) - 1
IOS_TITLE_ENTITY_LIST_NAME = "10939-ios_medium.csv"
GAME_ENTITY_LIST_NAME = "empty.csv"
ENTITY_LIST_DIRECTORIES = ("entities", "EntityLists")

STAGE_NAMES = (
    "health",
    "auth_login",
    "check_login",
    "sync_data",
    "wf_config",
    "leiting_guest_register",
    "leiting_login",
    "signup",
    "load_initial",
    "tutorial_skip",
    "asset_title_version",
    "asset_version",
    "asset_get_path",
    "asset_archive_download",
    "query_purcharge",
    "pass_card",
    "episode_trial_finish",
    "virtual_time",
    "single_battle_start",
    "single_battle_finish",
    "gacha_resource_mail_create",
    "gacha_resource_mail_receive",
    "gacha_pool_consistency",
    "gacha_single",
    "gacha_ten",
    "equipment_gacha",
    "multiplayer_ai_handshake",
    "mail_create",
    "mail_list",
    "mail_receive",
    "activity_catalog",
    "activity_close",
    "activity_open",
    "checkpoint",
    "load_persistence",
)

_MISSING = object()


# //// 表示一个可安全写入报告的场景失败 [@x380kkm 2026-08-21] ////
class ScenarioFailure(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


# //// 拒绝不满足协议断言的阶段 [@x380kkm 2026-08-21] ////
def require_scenario(condition, code, message):
    if not condition:
        raise ScenarioFailure(code, message)


# //// 读取嵌套协议字段 [@x380kkm 2026-08-21] ////
def _nested(value, *keys):
    current = value
    for key in keys:
        if not isinstance(current, dict) or key not in current:
            return _MISSING
        current = current[key]
    return current


# //// 按 JavaScript Number 语义读取协议数值 [@x380kkm 2026-08-21] ////
def _number(value):
    if value is _MISSING:
        return math.nan
    if value is None:
        return 0
    if isinstance(value, bool):
        return 1 if value else 0
    if isinstance(value, (int, float)):
        return value
    if isinstance(value, str):
        stripped = value.strip()
        if stripped == "":
            return 0
        try:
            return float(stripped)
        except ValueError:
            return math.nan
    return math.nan


# //// 读取协议数值并拒绝缺失字段 [@x380kkm 2026-08-21] ////
def _numeric_value(value, code, message):
    valid_type = isinstance(value, (int, float)) and not isinstance(value, bool)
    finite = valid_type and math.isfinite(value)
    safe = finite and (not isinstance(value, int) or abs(value) <= MAX_SAFE_INTEGER)
    require_scenario(safe, code, message)
    return value


# //// 判断转换后的协议数值是否为安全整数 [@x380kkm 2026-08-21] ////
def _is_safe_integer(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return False
    return math.isfinite(value) and float(value).is_integer() and abs(value) <= MAX_SAFE_INTEGER


# //// 判断文本是否为非空偶数字节十六进制 [@x380kkm 2026-08-22] ////
def _is_hex_text(value):
    return (
        isinstance(value, str)
        and len(value) > 0
        and len(value) % 2 == 0
        and all(character in "0123456789abcdefABCDEF" for character in value)
    )


# //// 读取普通角色扭蛋状态 [@x380kkm 2026-08-21] ////
def _gacha_info(data, gacha_id=80000):
    entries = _nested(data, "gacha_info_list")
    if not isinstance(entries, list):
        return _MISSING
    for entry in entries:
        if isinstance(entry, dict) and _number(_nested(entry, "gacha_id")) == gacha_id:
            return entry
    return _MISSING


# //// 执行严格有序的 CN 游戏场景阶段 [@x380kkm 2026-08-21] ////
class StartupScenarioStages:
    def __init__(
        self,
        request_json,
        request_cn,
        request_guest,
        request_asset_list,
        request_archive,
        device_id,
        sleep_impl=None,
        request_session=None,
    ):
        self.request_json = request_json
        self.request_cn = request_cn
        self.request_guest = request_guest
        self.request_asset_list = request_asset_list
        self.request_archive = request_archive
        self.request_session = request_session
        self.device_id = device_id
        self.sleep_impl = sleep_impl or (lambda milliseconds: time.sleep(milliseconds / 1000.0))
        self.viewer_id = None
        self.play_id = "ios-cn-scenario-%d" % os.getpid()
        self.battle_data = None
        self.gacha_draw_character_ids = []
        self.gacha_currency_before = None
        self.gacha_currency_after_single = None
        self.mail_mana_before = None
        self.final_expected_free_vmoney = None
        self.mail_id = None
        self.activity_id = None
        self.asset_archives = []
        self.title_asset_source_url = None
        self.tutorial_completed = False

    def health(self):
        body = self.request_json("GET", "/health")
        require_scenario(
            _nested(body, "status") == "ok",
            "HEALTH_INVALID",
            "the personal service health response is invalid",
        )
        generation = _number(_nested(body, "generation"))
        return {
            "service_healthy": True,
            "generation_present": isinstance(generation, (int, float))
            and math.isfinite(generation),
        }

    def _sdk_login(self, path):
        response = self.request_json("POST", path, {})
        require_scenario(
            _nested(response, "status") == "0" and _nested(response, "type") == "0",
            "SDK_LOGIN_STATUS_INVALID",
            "the SDK login response did not return the accepted guest state",
        )
        require_scenario(
            isinstance(_nested(response, "data"), str)
            and len(_nested(response, "data")) > 0,
            "SDK_LOGIN_DATA_INVALID",
            "the SDK login response did not return identity data",
        )
        return {"guest_identity_present": True}

    def auth_login(self):
        return self._sdk_login("/auth_login")

    def check_login(self):
        return self._sdk_login("/check_login")

    def sync_data(self):
        response = self.request_json("POST", "/sync_data", {})
        code = _nested(response, "code")
        require_scenario(
            isinstance(code, int) and not isinstance(code, bool) and code == 0,
            "SYNC_DATA_STATUS_INVALID",
            "sync data did not return code 0",
        )
        return {"sync_state_accepted": True}

    def wf_config(self):
        response = self.request_json("GET", "/wf/210009_config_20200415.json")
        token = _nested(response, "token")
        config = _nested(response, "config")
        require_scenario(
            _is_hex_text(token) and len(token) == 32,
            "WF_CONFIG_TOKEN_INVALID",
            "the WF configuration did not return a valid token",
        )
        require_scenario(
            _is_hex_text(config),
            "WF_CONFIG_BODY_INVALID",
            "the WF configuration did not return encrypted configuration data",
        )
        return {"configuration_present": True}

    def leiting_guest_register(self):
        response = self.request_guest()
        encoded_data = _nested(response, "data")
        try:
            decoded_data = base64.b64decode(encoded_data, validate=True)
        except (TypeError, ValueError, binascii.Error):
            decoded_data = b""
        require_scenario(
            _nested(response, "status") == "0",
            "LEITING_GUEST_STATUS_INVALID",
            "guest registration did not return status 0",
        )
        require_scenario(
            _nested(response, "type") == "0",
            "LEITING_GUEST_TYPE_INVALID",
            "guest registration did not return type 0",
        )
        require_scenario(
            isinstance(encoded_data, str) and len(decoded_data) > 0,
            "LEITING_GUEST_DATA_INVALID",
            "guest registration did not return encoded identity data",
        )
        return {"guest_registered": True, "encoded_identity_present": True}

    def leiting_login(self):
        response = self.request_cn(
            "/api/index.php/channels/channel_leiting/leiting_login",
            {"userId": "10000001"},
        )
        data = _nested(response, "data")
        require_scenario(
            _nested(data, "status") == "success",
            "LEITING_LOGIN_STATUS_INVALID",
            "CN leiting login did not return success",
        )
        require_scenario(
            _nested(data, "userId") == "10000001",
            "LEITING_LOGIN_IDENTITY_INVALID",
            "CN leiting login did not preserve the guest identity",
        )
        require_scenario(
            _nested(data, "data", "age") == 18
            and _nested(data, "data", "auth") == 1,
            "LEITING_LOGIN_AUTH_INVALID",
            "CN leiting login did not return the completed identity state",
        )
        require_scenario(
            _nested(data, "online_server_check") is True,
            "LEITING_LOGIN_SERVER_CHECK_INVALID",
            "CN leiting login did not enable the local server check",
        )
        return {"guest_identity_accepted": True, "identity_check_completed": True}

    def signup(self):
        response = self.request_cn(
            "/api/index.php/tool/signup",
            {"device_id": self.device_id},
        )
        viewer_id = _number(_nested(response, "data_headers", "viewer_id"))
        require_scenario(
            _is_safe_integer(viewer_id) and viewer_id > 0,
            "SIGNUP_VIEWER_INVALID",
            "signup did not assign an instance viewer",
        )
        login_token = _nested(response, "data", "login_token")
        require_scenario(
            isinstance(login_token, str) and len(login_token) > 0,
            "SIGNUP_CREDENTIAL_MISSING",
            "signup did not return a recovery credential",
        )
        self.viewer_id = int(viewer_id)
        return {
            "account_created_or_restored": True,
            "instance_identity_assigned": True,
        }

    def load_initial(self):
        response = self.request_cn(
            "/api/index.php/load",
            {"keychain": self.viewer_id, "viewer_id": self.viewer_id},
        )
        user_info = _nested(response, "data", "user_info")
        characters = _nested(response, "data", "user_character_list")
        require_scenario(
            isinstance(user_info, dict) and isinstance(characters, dict),
            "LOAD_SNAPSHOT_INCOMPLETE",
            "load did not return the required player snapshot fields",
        )
        self.tutorial_completed = _nested(response, "data", "user_tutorial") is None
        return {"player_loaded": True, "character_count": len(characters)}

    def tutorial_skip(self):
        if self.tutorial_completed:
            return {
                "skip_applied": False,
                "already_completed": True,
                "result_step": None,
            }
        response = self.request_cn(
            "/api/index.php/tutorial/update_step",
            {"viewer_id": self.viewer_id, "step": 0, "skip": True},
        )
        require_scenario(
            _number(_nested(response, "data", "step")) == 12,
            "TUTORIAL_SKIP_STEP_INVALID",
            "tutorial skip did not advance to the expected step",
        )
        require_scenario(
            _nested(response, "data", "mail_arrived") is True,
            "TUTORIAL_SKIP_STATE_INVALID",
            "tutorial skip did not return the expected state",
        )
        return {"skip_applied": True, "result_step": 12}

    def _asset_version(self, path, entity_list_name):
        response = self.request_cn(path, {}, {"DEVICE": "1", "res_ver": "1.4.8"})
        data = _nested(response, "data")
        base_url = _nested(data, "base_url")
        files_list = _nested(data, "files_list")
        require_scenario(
            isinstance(base_url, str) and len(base_url) > 0,
            "ASSET_VERSION_BASE_URL_INVALID",
            "asset version did not return a base URL",
        )
        normalized_base_url = base_url.rstrip("/")
        source_directory = normalized_base_url.rsplit("/", 1)[-1]
        require_scenario(
            source_directory in ENTITY_LIST_DIRECTORIES
            and files_list == "%s/%s" % (normalized_base_url, entity_list_name),
            "ASSET_VERSION_FILES_INVALID",
            "asset version did not return the expected local entity list",
        )
        total_size = _numeric_value(
            _nested(data, "total_size"),
            "ASSET_VERSION_SIZE_INVALID",
            "asset version did not return total_size",
        )
        require_scenario(
            total_size >= 0,
            "ASSET_VERSION_SIZE_INVALID",
            "asset version returned a negative total size",
        )
        entity_rows = self.request_asset_list(files_list)
        require_scenario(
            isinstance(entity_rows, list),
            "ASSET_VERSION_FILES_INVALID",
            "asset entity list could not be parsed as CSV",
        )
        return normalized_base_url, total_size, len(entity_rows)

    def asset_title_version(self):
        source_url, total_size, entity_count = self._asset_version(
            "/api/index.php/assetintitle/version_info_in_title",
            IOS_TITLE_ENTITY_LIST_NAME,
        )
        self.title_asset_source_url = source_url
        return {
            "entity_list_present": True,
            "entity_count": entity_count,
            "archive_bytes_present": total_size > 0,
        }

    def asset_version(self):
        source_url, total_size, entity_count = self._asset_version(
            "/api/index.php/asset/version_info",
            GAME_ENTITY_LIST_NAME,
        )
        require_scenario(
            source_url == self.title_asset_source_url,
            "ASSET_VERSION_MISMATCH",
            "title and game asset versions returned different entity list sources",
        )
        return {
            "entity_list_source_matches_title": True,
            "entity_count": entity_count,
            "archive_bytes_present": total_size > 0,
        }

    def _archive_metadata(self, archive):
        location = _nested(archive, "location")
        size = _number(_nested(archive, "size"))
        digest = _nested(archive, "sha256")
        require_scenario(
            isinstance(location, str) and size > 0,
            "ASSET_ARCHIVE_METADATA_INVALID",
            "asset path returned unusable archive metadata",
        )
        try:
            manifest_digest = base64.b64decode(digest, validate=True)
        except (TypeError, ValueError, binascii.Error):
            manifest_digest = b""
        require_scenario(
            len(manifest_digest) == 32,
            "ASSET_ARCHIVE_DIGEST_INVALID",
            "asset path did not return a valid archive digest",
        )
        return {
            "location": location,
            "size": int(size),
            "sha256": digest,
        }

    def asset_get_path(self):
        response = self.request_cn(
            "/api/index.php/asset/get_path",
            {"viewer_id": self.viewer_id},
            {"DEVICE": "1", "res_ver": "1.4.8"},
        )
        data = _nested(response, "data")
        require_scenario(
            _nested(data, "info", "client_asset_version") == "1.4.8",
            "ASSET_CLIENT_VERSION_INVALID",
            "asset path did not preserve the client resource version",
        )
        full_archives = _nested(data, "full", "archive")
        require_scenario(
            isinstance(full_archives, list) and len(full_archives) > 0,
            "ASSET_FULL_ARCHIVE_MISSING",
            "asset path did not return a full archive",
        )
        diff_groups = _nested(data, "diff")
        require_scenario(
            isinstance(diff_groups, list),
            "ASSET_DIFF_ARCHIVE_INVALID",
            "asset path did not return a diff archive list",
        )
        diff_archives = []
        for diff_group in diff_groups:
            archives = _nested(diff_group, "archive")
            require_scenario(
                isinstance(archives, list),
                "ASSET_DIFF_ARCHIVE_INVALID",
                "asset path returned an invalid diff archive group",
            )
            diff_archives.extend(archives)
        self.asset_archives = [
            self._archive_metadata(archive)
            for archive in full_archives + diff_archives
        ]
        return {
            "full_archive_count": len(full_archives),
            "diff_archive_count": len(diff_archives),
            "client_version_matched": True,
        }

    def asset_archive_download(self):
        for archive in self.asset_archives:
            status, content_type, size, signature, digest = self.request_archive(
                archive["location"]
            )
            require_scenario(
                200 <= status < 300,
                "ASSET_ARCHIVE_HTTP_%d" % status,
                "asset archive returned HTTP %d" % status,
            )
            require_scenario(
                content_type == "application/zip",
                "ASSET_ARCHIVE_CONTENT_TYPE_INVALID",
                "asset archive did not return ZIP content",
            )
            require_scenario(
                size == archive["size"] and size > 4,
                "ASSET_ARCHIVE_SIZE_INVALID",
                "asset archive size did not match its metadata",
            )
            require_scenario(
                signature == b"PK\x03\x04",
                "ASSET_ARCHIVE_SIGNATURE_INVALID",
                "asset archive did not contain a ZIP local header",
            )
            require_scenario(
                digest == archive["sha256"],
                "ASSET_ARCHIVE_DIGEST_MISMATCH",
                "asset archive digest did not match its manifest",
            )
        return {
            "archive_count": len(self.asset_archives),
            "zip_signatures_valid": True,
            "manifest_digests_matched": True,
        }

    def query_purcharge(self):
        response = self.request_cn(
            "/api/index.php/channels/channel_leiting_pay/query_purcharge",
            {},
        )
        require_scenario(
            _number(_nested(response, "data", "status")) == 3,
            "PURCHARGE_STATUS_INVALID",
            "purcharge query did not return the closed store state",
        )
        return {"store_closed_state_received": True}

    def pass_card(self):
        response = self.request_cn(
            "/api/index.php/Pass_card/get_pass_card",
            {},
        )
        data = _nested(response, "data")
        require_scenario(
            _number(_nested(data, "point")) == 0
            and _nested(data, "is_buy") is False
            and _nested(data, "all_received_record") == [],
            "PASS_CARD_STATE_INVALID",
            "Pass Card query did not return the empty local state",
        )
        return {"empty_pass_card_state_received": True}

    def episode_trial_finish(self):
        response = self.request_cn(
            "/api/index.php/episode_trial_reading/finish",
            {},
        )
        require_scenario(
            _nested(response, "data") == {},
            "EPISODE_TRIAL_STATE_INVALID",
            "episode trial finish did not return an empty state",
        )
        return {"episode_trial_finished": True}
