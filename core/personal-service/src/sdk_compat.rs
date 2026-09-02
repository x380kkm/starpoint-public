// audience: internal
// # sdk-compat
//
// 该模块为已确认的原生 SDK 请求返回客户端所需的媒体类型和最小正文.

use crate::http::{HttpRequest, HttpResponse};

const ADVERT_CONFIG_PATH: &str = "/logmonitor/api/advert!getNewConfig.action";
const ADVERT_CONFIG_BODY: &str = "{\"message\":\"advert_config\",\"status\":\"success\"}";
const BEHAVIOR_LOG_PATH: &str = "/behavior_log/report";
const BEHAVIOR_LOG_BODY: &str = "{\"message\":\"behavior_report\",\"status\":\"success\"}";
const CHAT_SDK_APP_INIT_PATH: &str = "/chat-sdk/sdk/user/v2/appInit.action";
const CHAT_SDK_APP_INIT_BODY: &str = r#"{"code":1,"data":{},"message":"","msg":""}"#;
const CHAT_SDK_CONFIG_PATH: &str = "/chat-sdk/sdk/user/v2/config.action";
const CHAT_SDK_CONFIG_BODY: &str = r#"{"code":1,"data":{"collectFlag":1,"companyId":"a56cc6541e3646c38f5c65185d9bba73","reqFrequency":2,"support":1},"message":"Hello world!","msg":"Hello world!"}"#;
const CITY_JSON_PATH: &str = "/cityjson";
const CITY_JSON_BODY: &str = r#"var returnCitySN = {"cip":"127.0.0.1","cid":"","cname":"local"};"#;
const CRASH_PATH: &str = "/crash";
const DEBUG_PATH: &str = "/debug";
const EMPTY_HTML_PATHS: &[&str] = &[
    "/account_destroy",
    "/account_retrieve/id_card.do",
    "/accountCenter/accountSelf/findPassWord",
    "/captcha/",
    "/chat/pc/v2/index.html",
    "/common/202108/survey/",
    "/game/common/feedback/",
    "/login/",
    "/msgrd",
    "/news/14778",
    "/pay/",
    "/sdk/",
    "/SDKCaptcha/AliCaptcha.html",
    "/utility/openApp.html",
];
const JAVASCRIPT_CONTENT_TYPE: &str = "application/javascript; charset=utf-8";
const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";
const IP138_PATH: &str = "/ips138.asp";
const IP138_BODY: &str = "<html><body><li>127.0.0.1</li></body></html>";
const LEITING_AUTH_QUALIFY_PATH: &str = "/api/auth_login/check_qualify";
const LEITING_AUTH_QUALIFY_BODY: &str = r#"{"status":"0","statusCode":"0","memo":"","message":"","data":"hKrvhn2UWGUCoOmpmya8ExlDwY7iPQmkyVcYfLqFgcY="}"#;
const LEITING_CAID_PATHS: &[&str] = &["/api/sdk_api!getCaidNew", "/api/sdk_api!getCaidNew.action"];
const LEITING_CAID_BODY: &str = r#"{"code":0,"data":{}}"#;
const LEITING_LOGIN_DEVICES_PATH: &str = "/aes/user/my_login_device";
const LEITING_LOGIN_DEVICES_BODY: &str =
    r#"{"status":"0","statusCode":"0","memo":"","message":"","data":"quomi09R8yWNqlZLp/eZ1g=="}"#;
const NO_CONTENT_REPORT_PATHS: &[&str] = &[
    "/ad_log/report",
    "/api/device/report",
    "/api/event_log/push",
    "/api/heartbeat!report.action",
    "/api/iplog/report",
    "/api/log_report!gdprAgreeLog.action",
    "/api/mg_log!addMgCreateRoleLog.action",
    "/api/mg_log!addMgLoginLog.action",
    "/api/mg_log!addMgRegisterLog.action",
    "/api/policy/report",
    "/api/sdk_log!addActivateLog.action",
    "/api/sdk_log!addRegisterLog.action",
    "/api/sdk_log!addScreenLog.action",
    "/api/sdklog/report",
    "/asa/adservices/api",
    "/game_docking_new/savelog",
    "/user/bind_cid",
    WF_CRASH_PATH,
];
const LEITING_LOGIN_PATHS: &[&str] = &[
    "/auth_login",
    "/check_login",
    "/mobile!mobileLoginPubV2.action",
    "/login/mobile!mobileLoginPubV2.action",
    "/mobile!sdkLogin.action",
    "/login/mobile!sdkLogin.action",
    "/mobile!guestRegister.action",
    "/login/mobile!guestRegister.action",
    "/mobile!sdkCheckLogin.action",
    "/login/mobile!sdkCheckLogin.action",
    "/sdk/v3-3/code_login_v2.do",
    "/sdk/v3-3/code_login.do",
    "/sdk/v3-3/pwd_login.do",
    "/sdk/v3-3/check_login.do",
    "/sdk/v3-3/check_force.do",
    "/sdk/v3-3/taptap_login.do",
    "/sdk/auth_login.do",
    "/sdk/v3-3/auth_login.do",
];
const LEITING_GUEST_LOGIN_BODY: &str = r#"{"status":"0","type":"0","message":"","data":"Ox8piDWnl7p3xCrJ3bwS8RSjUahG/oB4S8D+s39R7Bb/C7XfVkgxohfumfFMK/Or8Kppz+Bk/tZyrEHnERbc0NYeuBKFrcWdQ+gzSuuliP9kIb1uUBP9Uj0DxB49Pnr3MSs6FDp8SZXDvmPjKT8y0twAiSYGQu1GCUwpKT0uJH1zxb8Q6Zyj70UPLlRKPoKnsSRscBIlOj/ACkDy4cBCfAYFFTApjQY4+NnsddSYs40399y59OzTsKMGCuyghJeeBCeATZYeihAkkcj93Prd6YYI7jLYfUPDN4Rxlj5fx9d89ZKQcRE9GTophK7MWQdP6ihEfY49aUHvXXQjRlO3z+gAAhb2VPW8KHnmG/K0jds182SXYhY3EXqf9bPpbO8NqtYKOAx8lRQBO/h01yRP9vBITftZQ0PIee/27v4EsifUiNpGgZO0Z1nduxadLfZuScp+rsPO8LfXK9pc2LPq7Q86AuH80NfA7/zVnCzhvoOLf5G+KpyOtvlHnuVei76T0/clqHs2iBtrPv8vqlBxjeJo0g08dMbaZhYTsOrZv7Q0KSAd3lPrtI6EeB2PqNG1"}"#;
const LEITING_PHONE_CODE_PATHS: &[&str] = &[
    "/mobile_two!getRegisterCodeOnly.action",
    "/login/mobile_two!getRegisterCodeOnly.action",
    "/aes/message/send_phone_code",
    "/aes/message/send_login_verify_code",
    "/aes/message/send_bind_phone_login_code",
    "/aes/message/send_register_code",
];
const LEITING_STATUS_BODY: &str =
    r#"{"status":"0","statusCode":"0","memo":"","message":"","data":""}"#;
const MG_ACTIVATE_LOG_PATH: &str = "/api/mg_log!addMgActivateLog.action";
const MG_ACTIVATE_LOG_BODY: &str = "{\"message\":\"activate\",\"status\":\"success\"}";
const MICRO_RED_ENTER_POSITION_PATH: &str = "/api/micro/micro_red/enter_position";
const MICRO_RED_HIDDEN_BODY: &str = r#"{"code":2000,"data":{"isShow":false}}"#;
const MICRO_RED_LOGIN_INFO_PATH: &str = "/api/micro/login/enter_info";
const MY_IP_PATH: &str = "/myip";
const IP_COUNTRY_PATH: &str = "/ip/getIpContry.do";
const FAVICON_PATH: &str = "/favicon.ico";
const SOBOT_IMAGE_PATH: &str = "/chat/common/res/83f5636f-51b7-48d6-9d63-40eba0963bda.png";
const SOBOT_LOCALIZATION_PREFIX: &str = "/mobile/multilingual/ios/ios_";
const SKAN_QUERY_DETAIL_PATH: &str = "/api/skan/query_detail";
const SKAN_EXPIRED_SIGNATURE_BODY: &str = "{status=1, message=签名已过失效时间}";
const SYNC_DATA_PATH: &str = "/sync_data";
const SYNC_DATA_BODY: &str = "{\"code\":0}";
const TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
const TELEMETRY_OK_BODY: &[u8] = b"OK";
const TRANSPARENT_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];
const VIP_STATUS_PATH: &str = "/crm/vip/is_vip";
const VIP_STATUS_BODY: &str = r#"{"code":0,"data":{"is_vip":false}}"#;
const WF_CRASH_PATH: &str = "/wf_crash/crash.php";

// //// 标识 SDK 诊断路径 [@x380kkm 2026-08-23] ////
pub(crate) fn is_sdk_diagnostic_path(path: &str) -> bool {
    matches!(path, DEBUG_PATH | CRASH_PATH | WF_CRASH_PATH)
}
// //// /标识 SDK 诊断路径 ////

// //// 路由已确认的原生 SDK 请求 [@x380kkm 2026-08-23] ////
pub(crate) fn route(request: &HttpRequest) -> Option<HttpResponse> {
    route_method_path(request.method(), request.path())
}

fn route_method_path(method: &str, path: &str) -> Option<HttpResponse> {
    if method == "POST" && NO_CONTENT_REPORT_PATHS.contains(&path) {
        return Some(HttpResponse::empty("204 No Content"));
    }
    if method == "POST" && LEITING_CAID_PATHS.contains(&path) {
        return Some(no_store(HttpResponse::json(
            "200 OK",
            LEITING_CAID_BODY.to_owned(),
        )));
    }
    if LEITING_LOGIN_PATHS.contains(&path) {
        return Some(no_store(HttpResponse::json(
            "200 OK",
            LEITING_GUEST_LOGIN_BODY.to_owned(),
        )));
    }
    if LEITING_PHONE_CODE_PATHS.contains(&path) {
        return Some(no_store(HttpResponse::json(
            "200 OK",
            LEITING_STATUS_BODY.to_owned(),
        )));
    }
    if method == "GET" && is_empty_html_path(path) {
        return Some(no_store(HttpResponse::bytes(
            "200 OK",
            HTML_CONTENT_TYPE,
            Vec::new(),
        )));
    }
    if method == "GET" && is_sobot_localization_path(path) {
        return Some(no_store(HttpResponse::json("200 OK", "{}".to_owned())));
    }
    if (path == DEBUG_PATH && matches!(method, "GET" | "POST"))
        || (path == CRASH_PATH && method == "POST")
    {
        return Some(no_store(HttpResponse::bytes(
            "200 OK",
            TEXT_CONTENT_TYPE,
            TELEMETRY_OK_BODY.to_vec(),
        )));
    }

    match (method, path) {
        ("POST", LEITING_AUTH_QUALIFY_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            LEITING_AUTH_QUALIFY_BODY.to_owned(),
        ))),
        ("POST", LEITING_LOGIN_DEVICES_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            LEITING_LOGIN_DEVICES_BODY.to_owned(),
        ))),
        ("GET" | "POST", CHAT_SDK_CONFIG_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            CHAT_SDK_CONFIG_BODY.to_owned(),
        ))),
        ("POST", CHAT_SDK_APP_INIT_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            CHAT_SDK_APP_INIT_BODY.to_owned(),
        ))),
        ("POST", SYNC_DATA_PATH) => Some(HttpResponse::json("200 OK", SYNC_DATA_BODY.to_owned())),
        ("POST", ADVERT_CONFIG_PATH) => {
            Some(HttpResponse::json("200 OK", ADVERT_CONFIG_BODY.to_owned()))
        }
        ("POST", MG_ACTIVATE_LOG_PATH) => Some(HttpResponse::json(
            "200 OK",
            MG_ACTIVATE_LOG_BODY.to_owned(),
        )),
        ("POST", BEHAVIOR_LOG_PATH) => {
            Some(HttpResponse::json("200 OK", BEHAVIOR_LOG_BODY.to_owned()))
        }
        ("GET", MICRO_RED_ENTER_POSITION_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            MICRO_RED_HIDDEN_BODY.to_owned(),
        ))),
        ("GET" | "POST", MICRO_RED_LOGIN_INFO_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            MICRO_RED_HIDDEN_BODY.to_owned(),
        ))),
        ("GET", CITY_JSON_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            JAVASCRIPT_CONTENT_TYPE,
            CITY_JSON_BODY.as_bytes().to_vec(),
        ))),
        ("GET", MY_IP_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            TEXT_CONTENT_TYPE,
            b"127.0.0.1".to_vec(),
        ))),
        ("GET", IP_COUNTRY_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            TEXT_CONTENT_TYPE,
            b"127.0.0.1".to_vec(),
        ))),
        ("GET", IP138_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            HTML_CONTENT_TYPE,
            IP138_BODY.as_bytes().to_vec(),
        ))),
        ("GET" | "POST", VIP_STATUS_PATH) => Some(no_store(HttpResponse::json(
            "200 OK",
            VIP_STATUS_BODY.to_owned(),
        ))),
        ("GET", SOBOT_IMAGE_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            "image/png",
            TRANSPARENT_PNG.to_vec(),
        ))),
        ("GET" | "POST", DEBUG_PATH) | ("POST", CRASH_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            TEXT_CONTENT_TYPE,
            TELEMETRY_OK_BODY.to_vec(),
        ))),
        ("GET", FAVICON_PATH) => Some(HttpResponse::empty("204 No Content")),
        ("GET", "/") => Some(no_store(HttpResponse::bytes(
            "200 OK",
            HTML_CONTENT_TYPE,
            Vec::new(),
        ))),
        ("GET", SKAN_QUERY_DETAIL_PATH) => Some(no_store(HttpResponse::bytes(
            "200 OK",
            TEXT_CONTENT_TYPE,
            SKAN_EXPIRED_SIGNATURE_BODY.as_bytes().to_vec(),
        ))),
        _ => None,
    }
}

fn is_empty_html_path(path: &str) -> bool {
    EMPTY_HTML_PATHS.contains(&path)
        || path.starts_with("/terrace/")
        || (path.starts_with("/protocols/leiting/third/") && path.ends_with("/annex.html"))
}

fn is_sobot_localization_path(path: &str) -> bool {
    path.starts_with(SOBOT_LOCALIZATION_PREFIX) && path.ends_with(".json")
}

fn no_store(response: HttpResponse) -> HttpResponse {
    response
        .with_header("Cache-Control", "no-store")
        .with_header("X-Content-Type-Options", "nosniff")
}
// //// /路由已确认的原生 SDK 请求 ////

#[cfg(test)]
// //// 验证原生 SDK 兼容响应 [@x380kkm 2026-08-23] ////
mod tests {
    use super::*;
    use aes_gcm::aes::{
        cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
        Aes128,
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde_json::{json, Value};

    fn json_response_with_method(method: &str, path: &str) -> Value {
        let response = route_method_path(method, path).expect("route returns a response");
        assert!(response.is_success());
        assert_eq!(response.header("Content-Type"), Some("application/json"));
        assert_eq!(response.header("Cache-Control"), Some("no-store"));
        serde_json::from_slice(response.body()).expect("response body is JSON")
    }

    fn json_response(path: &str) -> Value {
        json_response_with_method("POST", path)
    }

    fn decrypt_leiting_data(response: &Value) -> Value {
        let encrypted = STANDARD
            .decode(response["data"].as_str().expect("data is encoded text"))
            .expect("data is Base64");
        assert_eq!(encrypted.len() % 16, 0);

        let cipher = Aes128::new_from_slice(b"#LeitingAESKey#!").expect("AES key has 16 bytes");
        let mut previous = *b"LeitingAESIVKEY!";
        let mut plaintext = Vec::with_capacity(encrypted.len());
        for encrypted_block in encrypted.chunks_exact(16) {
            let mut block = GenericArray::clone_from_slice(encrypted_block);
            cipher.decrypt_block(&mut block);
            plaintext.extend(
                block
                    .iter()
                    .zip(previous.iter())
                    .map(|(byte, previous_byte)| byte ^ previous_byte),
            );
            previous.copy_from_slice(encrypted_block);
        }

        let padding = *plaintext.last().expect("plaintext contains padding") as usize;
        assert!((1..=16).contains(&padding));
        assert!(plaintext[plaintext.len() - padding..]
            .iter()
            .all(|byte| *byte as usize == padding));
        plaintext.truncate(plaintext.len() - padding);
        serde_json::from_slice(&plaintext).expect("decrypted data is JSON")
    }

    #[test]
    fn returns_the_stable_leiting_guest_identity() {
        for path in LEITING_LOGIN_PATHS {
            let response = json_response(path);
            assert_eq!(response["status"], "0");
            assert_eq!(response["type"], "0");
            let guest = decrypt_leiting_data(&response);
            assert_eq!(guest["userId"], "10000001");
            assert_eq!(guest["userName"], "g_10000001");
            assert_eq!(guest["token"], "mock-session-token-0001");
            assert_eq!(guest["channelNo"], "110001");
            assert_eq!(guest["game"], "wf");
            assert_eq!(guest["isGuest"], "1");
            assert_eq!(guest["adult"], "1");
            assert_eq!(guest["statusCode"], "0");
            assert_eq!(json_response_with_method("GET", path), response);
        }
    }

    #[test]
    fn returns_success_for_phone_code_requests() {
        for path in LEITING_PHONE_CODE_PATHS {
            for method in ["GET", "POST"] {
                assert_eq!(
                    json_response_with_method(method, path),
                    json!({
                        "status": "0",
                        "statusCode": "0",
                        "memo": "",
                        "message": "",
                        "data": "",
                    })
                );
            }
        }
    }

    #[test]
    fn acknowledges_debug_and_crash_reports_as_text() {
        for (method, path) in [
            ("GET", DEBUG_PATH),
            ("POST", DEBUG_PATH),
            ("POST", CRASH_PATH),
        ] {
            let response = route_method_path(method, path).expect("route returns a response");
            assert!(response.is_success());
            assert_eq!(response.header("Content-Type"), Some(TEXT_CONTENT_TYPE));
            assert_eq!(response.body(), TELEMETRY_OK_BODY);
        }
        assert!(route_method_path("GET", CRASH_PATH).is_none());
    }

    #[test]
    fn returns_typed_leiting_login_prerequisites() {
        let qualify = json_response(LEITING_AUTH_QUALIFY_PATH);
        assert_eq!(qualify["status"], "0");
        assert_eq!(decrypt_leiting_data(&qualify), json!({"qualify": false}));

        let devices = json_response(LEITING_LOGIN_DEVICES_PATH);
        assert_eq!(devices["status"], "0");
        assert_eq!(decrypt_leiting_data(&devices), json!([]));
    }

    #[test]
    fn accepts_known_sdk_reports_without_response_bodies() {
        for path in NO_CONTENT_REPORT_PATHS {
            let response = route_method_path("POST", path).expect("route returns a response");
            assert!(response.is_success());
            assert!(response.body().is_empty());
            assert!(route_method_path("GET", path).is_none());
        }
    }

    #[test]
    fn returns_an_empty_caid_identity_for_known_sdk_variants() {
        for path in LEITING_CAID_PATHS {
            let response = json_response(path);
            assert_eq!(response, json!({"code": 0, "data": {}}));
            assert!(route_method_path("GET", path).is_none());
        }
    }

    #[test]
    fn suppresses_the_browser_favicon_lookup() {
        let response = route_method_path("GET", FAVICON_PATH).expect("route returns a response");
        assert!(response.is_success());
        assert!(response.body().is_empty());
        assert_eq!(response.header("Content-Type"), None);
        assert!(route_method_path("POST", FAVICON_PATH).is_none());
    }

    #[test]
    fn returns_a_hidden_micro_community_entry() {
        let response = route_method_path("GET", MICRO_RED_ENTER_POSITION_PATH)
            .expect("route returns a response");
        assert!(response.is_success());
        assert_eq!(response.header("Content-Type"), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Value>(response.body()).expect("response body is JSON"),
            json!({"code": 2000, "data": {"isShow": false}}),
        );
        assert!(route_method_path("POST", MICRO_RED_ENTER_POSITION_PATH).is_none());
    }

    #[test]
    fn returns_the_local_public_ip_script() {
        let response = route_method_path("GET", CITY_JSON_PATH).expect("route returns a response");
        assert!(response.is_success());
        assert_eq!(
            response.header("Content-Type"),
            Some(JAVASCRIPT_CONTENT_TYPE)
        );
        assert_eq!(response.body(), CITY_JSON_BODY.as_bytes());
        assert!(route_method_path("POST", CITY_JSON_PATH).is_none());
    }

    #[test]
    fn returns_the_known_chat_sdk_configuration() {
        for method in ["GET", "POST"] {
            let response = json_response_with_method(method, CHAT_SDK_CONFIG_PATH);
            assert_eq!(response["code"], 1);
            assert_eq!(response["data"]["collectFlag"], 1);
            assert_eq!(response["data"]["reqFrequency"], 2);
            assert_eq!(response["data"]["support"], 1);
        }
    }

    #[test]
    fn returns_a_closed_chat_sdk_session() {
        let response = json_response(CHAT_SDK_APP_INIT_PATH);
        assert_eq!(
            response,
            json!({"code": 1, "data": {}, "message": "", "msg": ""})
        );
        assert!(route_method_path("GET", CHAT_SDK_APP_INIT_PATH).is_none());
    }

    #[test]
    fn returns_typed_binary_sdk_resources() {
        let html = route_method_path("GET", "/account_destroy").expect("route returns a response");
        assert_eq!(html.header("Content-Type"), Some(HTML_CONTENT_TYPE));
        assert!(html.body().is_empty());

        let localization =
            json_response_with_method("GET", "/mobile/multilingual/ios/ios_zh_CN.json");
        assert_eq!(localization, json!({}));

        let image = route_method_path("GET", SOBOT_IMAGE_PATH).expect("route returns a response");
        assert_eq!(image.header("Content-Type"), Some("image/png"));
        assert!(image.body().starts_with(b"\x89PNG\r\n\x1a\n"));

        assert_eq!(
            json_response_with_method("GET", MICRO_RED_LOGIN_INFO_PATH),
            json!({"code": 2000, "data": {"isShow": false}}),
        );
        assert_eq!(
            json_response_with_method("GET", VIP_STATUS_PATH),
            json!({"code": 0, "data": {"is_vip": false}}),
        );

        let country = route_method_path("GET", IP_COUNTRY_PATH).expect("route returns a response");
        assert_eq!(country.header("Content-Type"), Some(TEXT_CONTENT_TYPE));
        assert_eq!(country.body(), b"127.0.0.1");

        let ip138 = route_method_path("GET", IP138_PATH).expect("route returns a response");
        assert_eq!(ip138.header("Content-Type"), Some(HTML_CONTENT_TYPE));
        assert_eq!(ip138.body(), IP138_BODY.as_bytes());
        assert!(route_method_path("POST", IP138_PATH).is_none());
    }
}
// //// /验证原生 SDK 兼容响应 ////
