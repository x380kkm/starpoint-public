// audience: internal
// # personal-service-player-web-tests
// 该文件验证普通玩家页面只提供玩家存档入口, 不嵌入管理 token 或管理员 API.

mod support;

use starpoint_personal_service::PersonalService;
use support::request;
use tempfile::TempDir;

//// 提供不携带管理凭据的普通玩家页面 [@x380kkm 2026-07-24] ////
#[test]
fn serves_player_assets_without_management_token() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let page = request(service.port(), "GET", "/player/?source=test");
    assert!(page.starts_with("HTTP/1.1 200 OK"));
    assert!(page.contains("Content-Type: text/html; charset=utf-8"));
    assert!(page.contains("Cache-Control: no-store"));
    assert!(page.contains("Content-Security-Policy: default-src 'none'"));
    assert!(page.contains("星点玩家存档"));
    assert!(page.contains("player-token-form"));
    assert!(page.contains("player-save-list"));
    assert!(page.contains("sync-upload-form"));
    assert!(page.contains("sync-download-form"));
    assert!(page.contains("recovery-export-form"));
    assert!(page.contains("recovery-import-form"));
    assert!(page.contains("/manage/style.css"));
    assert!(!page.contains(service.management_token()));

    let app = request(service.port(), "GET", "/player/app.js");
    assert!(app.starts_with("HTTP/1.1 200 OK"));
    assert!(app.contains("Content-Type: text/javascript; charset=utf-8"));
    assert!(app.contains("/v1/player/local-saves"));
    assert!(app.contains("encrypted-export"));
    assert!(app.contains("import-encrypted"));
    assert!(app.contains("/v1/player/save-sync-targets"));
    assert!(app.contains("/sync/upload"));
    assert!(app.contains("sync/download"));
    assert!(app.contains("/v1/player/recovery/export"));
    assert!(app.contains("/v1/player/recovery/import"));
    assert!(app.contains("只保存在模块内存中"));
    assert!(!app.contains("/v1/player-access"));
    assert!(!app.contains("/v1/server-profiles"));
    assert!(!app.contains(service.management_token()));

    let rejected = request(service.port(), "POST", "/player/");
    assert!(rejected.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(rejected.contains("Allow: GET"));

    service.stop().expect("service stops cleanly");
}
//// /提供不携带管理凭据的普通玩家页面 ////
