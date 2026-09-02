// audience: internal
// # personal-service-cn
//
// 该模块实现 CN 客户端账号前置协议. 主协议使用 base64 包装的 MessagePack.

use crate::cn_msgpack::normalize_client_msgpack_numbers;
use crate::cn_player;
use crate::database::{ServiceDatabase, SignupDeviceError, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{
    de::{self, DeserializeOwned, Deserializer, Visitor},
    Deserialize, Serialize,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

// //// 兼容 CN 客户端浮点 MessagePack 数字 [@x380kkm 2026-08-10] ////
const MAX_SAFE_INTEGER_F64: f64 = 9_007_199_254_740_991.0;

struct FlexibleI64Visitor;

impl<'de> Visitor<'de> for FlexibleI64Visitor {
    type Value = i64;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an integer or an integral MessagePack float")
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value).map_err(|_| E::custom("integer exceeds i64 range"))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value).map_err(|_| E::custom("integer exceeds i64 range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value).map_err(|_| E::custom("integer exceeds i64 range"))
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if !value.is_finite() || value.fract() != 0.0 || value.abs() > MAX_SAFE_INTEGER_F64 {
            return Err(E::custom("float is not a safe integer"));
        }
        Ok(value as i64)
    }
}

struct OptionalFlexibleI64Visitor;

impl<'de> Visitor<'de> for OptionalFlexibleI64Visitor {
    type Value = Option<i64>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an optional integer or integral MessagePack float")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleI64Visitor).map(Some)
    }
}

pub(crate) fn deserialize_optional_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(OptionalFlexibleI64Visitor)
}
// //// /兼容 CN 客户端浮点 MessagePack 数字 ////

#[derive(Serialize)]
struct DataHeaders {
    force_update: bool,
    asset_update: bool,
    short_udid: i64,
    viewer_id: i64,
    servertime: i64,
    result_code: i64,
}

#[derive(Serialize)]
struct Envelope<T> {
    data_headers: DataHeaders,
    data: T,
}

#[derive(Default, Deserialize)]
struct LoginRequest {
    #[serde(rename = "userId")]
    user_id: Option<String>,
}

#[derive(Default, Deserialize)]
struct SignupRequest {
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    device_id: Option<i64>,
}

#[derive(Default, Deserialize)]
struct ViewerRequest {
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    viewer_id: Option<i64>,
}

#[derive(Default, Deserialize)]
struct LoadRequest {
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    keychain: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    viewer_id: Option<i64>,
}

#[derive(Serialize)]
struct IdentityData {
    #[serde(rename = "idCard")]
    id_card: &'static str,
    age: i64,
    #[serde(rename = "isGuest")]
    is_guest: i64,
    auth: i64,
}

#[derive(Serialize)]
struct LoginData {
    status: &'static str,
    #[serde(rename = "userId")]
    user_id: String,
    data: IdentityData,
    online_server_check: bool,
    heart_beat_interval: i64,
}

#[derive(Serialize)]
struct AntiAddictionLimits {
    #[serde(rename = "onlineTime")]
    online_time: i64,
    #[serde(rename = "limitTime")]
    limit_time: i64,
    #[serde(rename = "usableTime")]
    usable_time: i64,
}

#[derive(Serialize)]
struct AntiAddictionData {
    status: i64,
    message: &'static str,
    data: AntiAddictionLimits,
}

#[derive(Serialize)]
struct SignupData {
    login_token: String,
    #[serde(rename = "newAccount")]
    new_account: i64,
    #[serde(rename = "roleName")]
    role_name: String,
    #[serde(rename = "accountName")]
    account_name: String,
    sign: &'static str,
    #[serde(rename = "createDate")]
    create_date: String,
    #[serde(rename = "serverName")]
    server_name: &'static str,
    #[serde(rename = "serverId")]
    server_id: i64,
}

#[derive(Serialize)]
struct EnableData {
    enable: bool,
}

#[derive(Serialize)]
struct GiftData {
    enable_gift: bool,
}

#[derive(Serialize)]
struct ContactData {
    enable_customer_service: bool,
}

// //// 解码 CN 服务请求正文 [@x380kkm 2026-07-22] ////
pub(crate) fn decode_request<T: DeserializeOwned>(
    request: &HttpRequest,
) -> Result<T, HttpResponse> {
    let content_type = request.header("content-type").unwrap_or_default();
    if content_type.starts_with("application/json") {
        return serde_json::from_slice(request.body()).map_err(|_| invalid_request_body());
    }

    if !content_type.starts_with("application/x-www-form-urlencoded") {
        return Err(invalid_request_body());
    }

    let encoded = std::str::from_utf8(request.body())
        .map(str::trim)
        .map_err(|_| invalid_request_body())?;
    if let Ok(packed) = STANDARD.decode(encoded) {
        if let Ok(decoded) = rmp_serde::from_slice(&packed) {
            return Ok(decoded);
        }
    }

    let fields = url::form_urlencoded::parse(encoded.as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    let deserializer =
        serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(fields.into_iter());
    T::deserialize(deserializer).map_err(|_| invalid_request_body())
}
// //// /解码 CN 服务请求正文 ////

// //// 编码服务端使用的 base64 MessagePack 响应 [@x380kkm 2026-07-22] ////
pub(crate) fn msgpack_response<T: Serialize>(
    database: &ServiceDatabase,
    viewer_id: i64,
    asset_update: bool,
    data: T,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response_at(
        viewer_id,
        asset_update,
        database.current_server_time_seconds()?,
        data,
    )
}

pub(crate) fn msgpack_response_at<T: Serialize>(
    viewer_id: i64,
    asset_update: bool,
    response_time: i64,
    data: T,
) -> Result<HttpResponse, PersonalServiceError> {
    encode_msgpack_response(viewer_id, asset_update, response_time, 1, data)
}

pub(crate) fn msgpack_result_code_response_at(
    viewer_id: i64,
    response_time: i64,
    result_code: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    encode_msgpack_response(
        viewer_id,
        false,
        response_time,
        result_code,
        BTreeMap::<String, String>::new(),
    )
}

pub(crate) fn json_response_at<T: Serialize>(
    viewer_id: i64,
    asset_update: bool,
    response_time: i64,
    data: T,
) -> Result<HttpResponse, PersonalServiceError> {
    let envelope = Envelope {
        data_headers: DataHeaders {
            force_update: false,
            asset_update,
            short_udid: 0,
            viewer_id,
            servertime: response_time,
            result_code: 1,
        },
        data,
    };
    serde_json::to_string(&envelope)
        .map(|body| HttpResponse::json("200 OK", body))
        .map_err(|error| {
            PersonalServiceError::new(format!("failed to encode CN JSON response: {error}"))
        })
}

fn encode_msgpack_response<T: Serialize>(
    viewer_id: i64,
    asset_update: bool,
    response_time: i64,
    result_code: i64,
    data: T,
) -> Result<HttpResponse, PersonalServiceError> {
    let envelope = Envelope {
        data_headers: DataHeaders {
            force_update: false,
            asset_update,
            short_udid: 0,
            viewer_id,
            servertime: response_time,
            result_code,
        },
        data,
    };
    let packed = rmp_serde::to_vec_named(&envelope).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode CN response: {error}"))
    })?;
    let packed = normalize_client_msgpack_numbers(&packed)?;
    Ok(HttpResponse::bytes(
        "200 OK",
        "application/x-msgpack",
        STANDARD.encode(packed).into_bytes(),
    ))
}
// //// /编码服务端使用的 base64 MessagePack 响应 ////

fn invalid_request_body() -> HttpResponse {
    error_response("400 Bad Request", "invalid_request_body")
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

pub(crate) fn server_time(database: &ServiceDatabase) -> Result<i64, PersonalServiceError> {
    database.current_server_time_seconds()
}

// //// 生成 CN 登录响应使用的随机令牌 [@x380kkm 2026-07-22] ////
fn generate_login_token() -> Result<String, PersonalServiceError> {
    const TOKEN_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut random_bytes = [0_u8; 32];
    getrandom::getrandom(&mut random_bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate login token: {error}"))
    })?;
    Ok(random_bytes
        .iter()
        .map(|value| TOKEN_ALPHABET[*value as usize % TOKEN_ALPHABET.len()] as char)
        .collect())
}
// //// /生成 CN 登录响应使用的随机令牌 ////

// //// 处理雷霆账号兼容入口 [@x380kkm 2026-07-22] ////
fn route_leiting(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/channels/channel_leiting/leiting_login" => {
            let body = match decode_request::<LoginRequest>(request) {
                Ok(body) => body,
                Err(response) => return Some(Ok(response)),
            };
            msgpack_response(
                database,
                0,
                false,
                LoginData {
                    status: "success",
                    user_id: body.user_id.unwrap_or_default(),
                    data: IdentityData {
                        id_card: "123456",
                        age: 18,
                        is_guest: 0,
                        auth: 1,
                    },
                    online_server_check: true,
                    heart_beat_interval: 240,
                },
            )
        }
        "/api/index.php/channels/channel_leiting/leiting_antiaddiction_login" => msgpack_response(
            database,
            0,
            false,
            AntiAddictionData {
                status: 0,
                message: "success",
                data: AntiAddictionLimits {
                    online_time: 0,
                    limit_time: 999_999,
                    usable_time: 999_999,
                },
            },
        ),
        "/api/index.php/channels/channel_leiting/leiting_antiaddiction_logout"
        | "/api/index.php/channels/channel_leiting/leiting_update" => {
            msgpack_response(database, 0, false, BTreeMap::<String, i64>::new())
        }
        _ => return None,
    };
    Some(response)
}
// //// /处理雷霆账号兼容入口 ////

// //// 处理雷霆支付查询兼容入口 [@x380kkm 2026-08-22] ////
fn route_leiting_payment(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/channels/channel_leiting_pay/query_unfinish_order" => {
            msgpack_response(database, 0, false, BTreeMap::from([("order_id", "")]))
        }
        "/api/index.php/channels/channel_leiting_pay/query_purcharge" => {
            msgpack_response(database, 0, false, BTreeMap::from([("status", 3_i64)]))
        }
        "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_status"
        | "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_error" => {
            msgpack_response(database, 0, false, BTreeMap::<String, i64>::new())
        }
        _ => return None,
    };
    Some(response)
}
// //// /处理雷霆支付查询兼容入口 ////

// //// 处理 CN 工具账号入口 [@x380kkm 2026-07-22] ////
fn route_tool(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/tool/get_header_response" => {
            let body = match decode_request::<ViewerRequest>(request) {
                Ok(body) => body,
                Err(response) => return Some(Ok(response)),
            };
            msgpack_response(
                database,
                body.viewer_id.unwrap_or_default(),
                false,
                Vec::<i64>::new(),
            )
        }
        "/api/index.php/tool/auth" | "/api/index.php/tool/custom_notify" => {
            msgpack_response(database, 0, false, BTreeMap::<String, i64>::new())
        }
        "/api/index.php/tool/signup" => {
            let body = match decode_request::<SignupRequest>(request) {
                Ok(body) => body,
                Err(response) => return Some(Ok(response)),
            };
            let device_id = match body.device_id {
                Some(device_id) if device_id > 0 => device_id,
                _ => return Some(Ok(error_response("400 Bad Request", "invalid_device_id"))),
            };
            let login_token = match generate_login_token() {
                Ok(login_token) => login_token,
                Err(error) => return Some(Err(error)),
            };
            let default_server_time = match server_time(database) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
            let default_client_time = match database.get_current_client_time() {
                Ok(client_time) => client_time,
                Err(error) => return Some(Err(error)),
            };
            let default_player_data = match cn_player::create_default_player_data(
                default_server_time,
                &default_client_time,
            ) {
                Ok(player_data) => player_data,
                Err(error) => return Some(Err(error)),
            };
            let signup = match database
                .get_or_create_account_and_rotate_viewer_session(device_id, &default_player_data)
            {
                Ok(signup) => signup,
                Err(SignupDeviceError::BindingConflict) => {
                    return Some(Ok(error_response(
                        "409 Conflict",
                        "device_binding_conflict",
                    )))
                }
                Err(SignupDeviceError::Storage(error)) => return Some(Err(error)),
            };
            let account_name = format!("Player{}", signup.account_id);
            let data = SignupData {
                login_token,
                new_account: if signup.is_new { 1 } else { 0 },
                role_name: account_name.clone(),
                account_name,
                sign: "dummy_sign",
                create_date: signup.created_at,
                server_name: "StarPoint CN",
                server_id: 1,
            };
            msgpack_response(database, signup.viewer_id, false, data)
        }
        "/api/index.php/tool/check_social_link_enable" => {
            msgpack_response(database, 0, false, EnableData { enable: false })
        }
        "/api/index.php/tool/check_enable_gift" => {
            msgpack_response(database, 0, false, GiftData { enable_gift: true })
        }
        "/api/index.php/tool/contact_active"
        | "/api/index.php/tool/check_enable_customer_service" => msgpack_response(
            database,
            0,
            false,
            ContactData {
                enable_customer_service: false,
            },
        ),
        _ => return None,
    };
    Some(response)
}
// //// /处理 CN 工具账号入口 ////

// //// 校验 CN 载入请求并返回持久化玩家数据 [@x380kkm 2026-08-23] ////
fn route_load(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &std::path::Path,
    override_root: &std::path::Path,
    asset_digest_cache: &mut crate::cn_asset::ArchiveDigestCache,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.path() != "/api/index.php/load" {
        return None;
    }
    let body = match decode_request::<LoadRequest>(request) {
        Ok(body) => body,
        Err(response) => return Some(Ok(response)),
    };
    let viewer_id = match body
        .viewer_id
        .filter(|viewer_id| *viewer_id > 0)
        .or_else(|| body.keychain.filter(|viewer_id| *viewer_id > 0))
    {
        Some(viewer_id) => viewer_id,
        _ => return Some(Ok(error_response("400 Bad Request", "invalid_viewer_id"))),
    };
    let response_time = match server_time(database) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let client_time = match database.get_current_client_time() {
        Ok(client_time) => client_time,
        Err(error) => return Some(Err(error)),
    };
    let snapshot = match database.lookup_viewer_session_player(viewer_id) {
        Ok(ViewerSessionPlayer::InvalidSession) => {
            return Some(Ok(error_response(
                "400 Bad Request",
                "invalid_viewer_session",
            )))
        }
        Ok(ViewerSessionPlayer::MissingPlayer) => {
            return Some(Ok(error_response("400 Bad Request", "no_player")))
        }
        Ok(ViewerSessionPlayer::MissingPlayerData(_)) => {
            return Some(Ok(error_response(
                "500 Internal Server Error",
                "no_player_data",
            )))
        }
        Ok(ViewerSessionPlayer::Present(snapshot)) => snapshot,
        Err(error) => return Some(Err(error)),
    };
    let unfinished_quest = match database.get_unfinished_quest(snapshot.account_id) {
        Ok(unfinished_quest) => unfinished_quest,
        Err(error) => return Some(Err(error)),
    };
    let mut player_data = match cn_player::prepare_player_data(
        &snapshot.data,
        viewer_id,
        database,
        asset_root,
        override_root,
        request.header("res_ver"),
        crate::cn_asset::request_platform(request),
        asset_digest_cache,
        response_time,
        &client_time,
        unfinished_quest.as_ref(),
    ) {
        Ok(player_data) => player_data,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = crate::cn_reference_state_misc::remove_unknown_active_missions_from_load(
        &mut player_data.response,
    ) {
        return Some(Err(error));
    }
    if let Err(error) = crate::cn_gacha::refresh_daily_availability(
        &mut player_data.response,
        database,
        snapshot.account_id,
    ) {
        return Some(Err(error));
    }
    if let Err(error) = crate::cn_mission::inject_load_awake_summary(
        &mut player_data.response,
        &player_data.snapshot,
        database,
        snapshot.account_id,
    ) {
        return Some(Err(error));
    }
    if let Err(error) =
        crate::cn_mana::project_load_mana_board_state(&mut player_data.response, response_time)
    {
        return Some(Err(error));
    }
    let mail_arrived = match database.has_unreceived_mail(snapshot.account_id, response_time) {
        Ok(mail_arrived) => mail_arrived,
        Err(error) => return Some(Err(error)),
    };
    if let Some(root) = player_data.response.as_object_mut() {
        let host = request.header("host").unwrap_or("127.0.0.1");
        root.insert(
            "cn_crash_url".to_owned(),
            Value::String(format!("http://{host}/crash")),
        );
        root.insert("mail_arrived".to_owned(), Value::Bool(mail_arrived));
    }
    if let Err(error) = database.save_player_snapshot(snapshot.account_id, &player_data.snapshot) {
        return Some(Err(error));
    }
    Some(msgpack_response_at(
        viewer_id,
        true,
        response_time,
        player_data.response,
    ))
}
// //// /校验 CN 载入请求并返回持久化玩家数据 ////

// //// 分派 CN 客户端账号前置请求 [@x380kkm 2026-07-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &std::path::Path,
    override_root: &std::path::Path,
    asset_digest_cache: &mut crate::cn_asset::ArchiveDigestCache,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() == "GET" {
        return crate::cn_reference_state_misc::route(request, database);
    }
    if request.method() != "POST" {
        return None;
    }
    if let Some(response) = route_leiting(request, database) {
        return Some(response);
    }
    if let Some(response) = route_leiting_payment(request, database) {
        return Some(response);
    }
    if let Some(response) = route_tool(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_gacha::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_box_gacha::route(request, database, asset_root) {
        return Some(response);
    }
    if let Some(response) = crate::cn_shop::route(request, database, asset_root) {
        return Some(response);
    }
    if let Some(response) = crate::cn_exchange::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_multi_special_exchange::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_ex_boost::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_activity::route(request, database, asset_root) {
        return Some(response);
    }
    if let Some(response) = crate::cn_character::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_equipment::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_mana::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_party::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_party_group::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_profile::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_quest::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_expod::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_option::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_pass_card::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_payment::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_auxiliary::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_optional_exchange::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_news::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_reference_read::route(request, database, asset_root) {
        return Some(response);
    }
    if let Some(response) = crate::cn_reference_state_misc::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_story::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_episode_trial_reading::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_mail::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_mission::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_tutorial::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_multi::route(request, database) {
        return Some(response);
    }
    if let Some(response) = crate::cn_battle::route(request, database) {
        return Some(response);
    }
    route_load(
        request,
        database,
        asset_root,
        override_root,
        asset_digest_cache,
    )
}
// //// /分派 CN 客户端账号前置请求 ////

// //// 验证 CN 浮点 MessagePack 数字解码 [@x380kkm 2026-08-10] ////
#[cfg(test)]
mod tests {
    use super::SignupRequest;

    fn float_signup_request(value: f64) -> Vec<u8> {
        let mut packed = vec![0x81, 0xa9];
        packed.extend_from_slice(b"device_id");
        packed.push(0xcb);
        packed.extend_from_slice(&value.to_be_bytes());
        packed
    }

    #[test]
    fn accepts_an_integral_float_device_id() {
        let request: SignupRequest =
            rmp_serde::from_slice(&float_signup_request(123_456_789.0)).expect("request decodes");
        assert_eq!(request.device_id, Some(123_456_789));
    }

    #[test]
    fn rejects_a_fractional_float_device_id() {
        let result = rmp_serde::from_slice::<SignupRequest>(&float_signup_request(1.5));
        assert!(result.is_err());
    }
}
// //// /验证 CN 浮点 MessagePack 数字解码 ////
