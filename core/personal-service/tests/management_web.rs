// audience: internal
// # personal-service-management-web-tests
//
// 该文件验证静态管理页面, 安全响应头和 loopback 直接管理入口.

mod support;

use starpoint_personal_service::PersonalService;
use std::fs;
use support::{request, request_bytes};
use tempfile::TempDir;

// //// 提供打开即用的本机管理页面 [@x380kkm 2026-08-20] ////
#[test]
fn serves_management_assets_without_a_management_unlock_step() {
    let root = TempDir::new().expect("temporary service directory is created");
    let icon_directory = root.path().join("cdn/cn/management-assets/item-icons");
    fs::create_dir_all(&icon_directory).expect("item icon directory is created");
    fs::write(
        icon_directory.join("stamina.recovery.large.png"),
        b"\x89PNG\r\n\x1a\n",
    )
    .expect("item icon is written");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let page = request(service.port(), "GET", "/manage/?source=test");
    assert!(page.starts_with("HTTP/1.1 200 OK"));
    assert!(page.contains("Content-Type: text/html; charset=utf-8"));
    assert!(page.contains("Cache-Control: no-store"));
    assert!(page.contains("X-Content-Type-Options: nosniff"));
    assert!(page.contains("Referrer-Policy: no-referrer"));
    assert!(page.contains("Content-Security-Policy: default-src 'none'"));
    assert!(page.contains("星点个人服务"));
    assert!(page.contains("role=\"tablist\" aria-label=\"管理功能\""));
    assert_eq!(page.matches("role=\"tab\"").count(), 4);
    assert_eq!(page.matches("role=\"tabpanel\"").count(), 4);
    for tab_name in ["activity", "ai", "server", "other"] {
        let tab_id = format!("id=\"management-tab-{tab_name}\"");
        let tab_start = page.find(&tab_id).expect("management tab exists");
        let tab_end = page[tab_start..]
            .find('>')
            .map(|offset| tab_start + offset)
            .expect("management tab start tag is complete");
        let tab = &page[tab_start..=tab_end];
        assert!(tab.contains("role=\"tab\""));
        assert!(tab.contains(&format!("aria-controls=\"management-panel-{tab_name}\"")));
        assert!(tab.contains(&format!("data-management-tab=\"{tab_name}\"")));

        let panel_id = format!("id=\"management-panel-{tab_name}\"");
        let panel_start = page.find(&panel_id).expect("management tab panel exists");
        let panel_end = page[panel_start..]
            .find('>')
            .map(|offset| panel_start + offset)
            .expect("management tab panel start tag is complete");
        let panel = &page[panel_start..=panel_end];
        assert!(panel.contains("role=\"tabpanel\""));
        assert!(panel.contains(&format!("aria-labelledby=\"management-tab-{tab_name}\"")));
        assert!(panel.contains(&format!("data-management-tab-panel=\"{tab_name}\"")));
    }
    assert!(page.contains("虚拟服务器时间"));
    assert!(page.contains("活动目录"));
    assert!(page.contains("activity-search"));
    assert!(page.contains("activity-calendar"));
    assert!(page.contains("activity-favorite-filter"));
    assert!(page.contains("activity-detail-banner"));
    assert!(page.contains("activity-mode-form"));
    assert!(page.contains("activity-window-form"));
    assert!(page.contains("activity-period-form"));
    assert!(page.contains("activity-open-button"));
    assert!(page.contains("开放 24 小时"));
    assert!(page.contains("activity-calendar-today"));
    assert!(page.contains("id=\"activity-reset\""));
    assert!(page.contains("id=\"activityQuickFilters\""));
    assert!(page.contains("id=\"activityTagFilters\""));
    assert!(page.contains("每条请求显示最新状态, 展开可查看历史状态"));
    assert!(page.contains(
        "id=\"activity-period-interval\" name=\"interval_days\" type=\"number\" min=\"1\" max=\"3650\""
    ));
    assert!(page.contains("Raid Boss 状态"));
    assert!(page.contains("AI 编队"));
    assert!(page.contains("ai-team-slot"));
    assert!(page.contains("ai-team-a"));
    assert!(page.contains("ai-team-b"));
    assert!(page.contains("ai-team-candidates"));
    assert!(page.contains("activity-form"));
    assert!(page.contains("发放本地邮件"));
    assert!(page.contains("mail-form"));
    assert!(page.contains("mail-slot-id"));
    assert!(page.contains("mail-reward-search"));
    assert!(page.contains("mail-reward-favorites"));
    assert!(page.contains("mail-preset-list"));
    assert!(page.contains("player-access-form"));
    assert!(page.contains("存档访问授权"));
    assert!(page.contains("id=\"remote-save-panel\" class=\"panel\" hidden"));
    assert!(page
        .contains("id=\"remote-player-access-panel\" class=\"panel player-access-panel\" hidden"));
    assert!(!page.contains("密文通道"));
    assert!(!page.contains("奖励 JSON"));
    assert!(page.contains("type=\"module\""));
    assert!(!page.contains("解锁管理界面"));
    assert!(!page.contains("管理 token"));
    assert!(!page.contains(service.management_token()));
    assert!(page.contains("id=\"shop-stock-form\""));
    assert!(page.contains("id=\"shop-stock-slot-id\""));
    assert!(page.contains("id=\"shop-stock-viewer-id\""));
    assert!(page.contains("id=\"shop-stock-type\""));
    assert!(page.contains("id=\"shop-stock-item-id\""));
    assert!(page.contains("刷新库存"));

    let app = request(service.port(), "GET", "/manage/app.js");
    assert!(app.starts_with("HTTP/1.1 200 OK"));
    assert!(app.contains("Content-Type: text/javascript; charset=utf-8"));
    assert!(app.contains("history.replaceState"));
    assert!(app.contains("/automation"));
    assert!(app.contains("/v1/time"));
    assert!(app.contains("/v1/local-saves/"));
    assert!(app.contains("/context"));
    assert!(app.contains("/mails"));
    assert!(app.contains("/v1/player-access"));
    assert!(app.contains("/v1/shop-stock/refresh"));
    assert!(app.contains("renderShopStockSlotContexts"));
    assert!(app.contains("updateShopStockViewer"));
    assert!(app.contains("/v1/activities/raid-boss/"));
    assert!(app.contains("createActivityController"));
    assert!(app.contains("createAiTeamController"));
    assert!(app.contains("createMailRewardController"));
    assert!(app.contains("requestApi,"));
    assert!(app.contains("managementTabs: [...document.querySelectorAll"));
    assert!(app.contains("managementTabPanels: [...document.querySelectorAll"));
    assert!(app.contains("tab.setAttribute(\"aria-selected\", String(selected))"));
    assert!(app.contains("handleManagementTabKeydown"));
    assert!(app.contains("sessionStorage.getItem(managementTabSessionKey)"));
    assert!(app.contains("activityQuickFilters: document.querySelector(\"#activityQuickFilters\")"));
    assert!(app.contains("activityTagFilters: document.querySelector(\"#activityTagFilters\")"));
    assert!(!app.contains("Authorization: `Bearer"));
    assert!(!app.contains("token-form"));
    assert!(!app.contains(service.management_token()));

    let ai_team_controller = request(service.port(), "GET", "/manage/ai-team-controller.js");
    assert!(ai_team_controller.starts_with("HTTP/1.1 200 OK"));
    assert!(ai_team_controller.contains("export function createAiTeamController"));
    assert!(ai_team_controller.contains("/ai-teams"));
    assert!(ai_team_controller.contains("party_ids"));
    assert!(ai_team_controller.contains("两个 AI 必须使用不同编队"));

    let views = request(service.port(), "GET", "/manage/views.js");
    assert!(views.starts_with("HTTP/1.1 200 OK"));
    assert!(views.contains("export function renderManagement"));
    assert!(views.contains("设置自动快照"));
    assert!(views.contains("/probe"));
    assert!(views.contains("/activate-verified"));
    assert!(views.contains("检测并切换"));
    assert!(views.contains("navigator.share"));
    assert!(views.contains("shareOrDownloadJson"));
    assert!(views.contains("groupHttpObservations"));
    assert!(views.contains("dataset.currentStatus"));
    assert!(views.contains("查看历史状态"));

    let mail_reward_controller =
        request(service.port(), "GET", "/manage/mail-reward-controller.js");
    assert!(mail_reward_controller.starts_with("HTTP/1.1 200 OK"));
    assert!(mail_reward_controller.contains("export function createMailRewardController"));
    assert!(mail_reward_controller.contains("/v1/mail-rewards/catalog"));
    assert!(mail_reward_controller.contains("/favorite"));
    assert!(mail_reward_controller.contains("/manage/assets/item-placeholder.svg"));
    assert!(!mail_reward_controller.contains("item.name.slice(0, 1)"));

    let placeholder = request(service.port(), "GET", "/manage/assets/item-placeholder.svg");
    assert!(placeholder.starts_with("HTTP/1.1 200 OK"));
    assert!(placeholder.contains("Content-Type: image/svg+xml"));
    assert!(!placeholder.contains("<text"));

    let item_icon = request_bytes(
        service.port(),
        "GET",
        "/manage/assets/item-icons/stamina.recovery.large.png",
    );
    let header_end = item_icon
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("item icon response contains HTTP headers");
    let headers = std::str::from_utf8(&item_icon[..header_end])
        .expect("item icon response headers are valid UTF-8");
    assert!(headers.starts_with("HTTP/1.1 200 OK"));
    assert!(headers.contains("Content-Type: image/png"));
    assert!(item_icon[(header_end + 4)..].starts_with(b"\x89PNG\r\n\x1a\n"));
    let embedded_icon = request_bytes(
        service.port(),
        "GET",
        "/manage/assets/item-icons/currency.free-vmoney.png",
    );
    let embedded_header_end = embedded_icon
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("embedded item icon response contains HTTP headers");
    let embedded_headers = std::str::from_utf8(&embedded_icon[..embedded_header_end])
        .expect("embedded item icon response headers are valid UTF-8");
    assert!(embedded_headers.starts_with("HTTP/1.1 200 OK"));
    assert!(embedded_headers.contains("Content-Type: image/png"));
    assert!(embedded_icon[(embedded_header_end + 4)..].starts_with(b"\x89PNG\r\n\x1a\n"));
    let rejected_icon = request(
        service.port(),
        "POST",
        "/manage/assets/item-icons/stamina.recovery.large.png",
    );
    assert!(rejected_icon.starts_with("HTTP/1.1 405 Method Not Allowed"));
    let unsafe_icon = request(
        service.port(),
        "GET",
        "/manage/assets/item-icons/..%2Fdatabase.png",
    );
    assert!(unsafe_icon.starts_with("HTTP/1.1 404 Not Found"));

    let activity_controller = request(service.port(), "GET", "/manage/activity-controller.js");
    assert!(activity_controller.starts_with("HTTP/1.1 200 OK"));
    assert!(activity_controller.contains("export function createActivityController"));
    assert!(activity_controller.contains("/v1/activities/catalog"));
    assert!(activity_controller.contains("/v1/activities/reset"));
    assert!(activity_controller.contains("/favorite"));
    assert!(activity_controller.contains("/close"));
    assert!(activity_controller.contains("/mode"));
    assert!(activity_controller.contains("/window"));
    assert!(activity_controller.contains("/period"));
    assert!(activity_controller.contains("/temporary-open"));
    assert!(activity_controller.contains("image_candidates"));
    assert!(activity_controller.contains("setCurrentTime"));
    assert!(activity_controller.contains("setActivityDate(now.toISOString().slice(0, 10))"));
    assert!(activity_controller.contains("renderActivityDetail(state, elements, actions)"));
    assert!(activity_controller.contains("setActivityStatusFilter"));
    assert!(activity_controller.contains("setActivityTagFilter"));
    assert!(activity_controller.contains("clearActivityFilters"));

    let activity_views = request(service.port(), "GET", "/manage/activity-views.js");
    assert!(activity_views.starts_with("HTTP/1.1 200 OK"));
    assert!(activity_views.contains("export function renderActivityCatalog"));
    assert!(activity_views.contains("export function renderActivityDetail"));
    assert!(activity_views.contains("/manage/assets/activity-banners/"));
    assert!(activity_views.contains("safeActivityImageUrl"));
    assert!(activity_views.contains("activityImageCandidates"));
    assert!(activity_views.contains("renderActivityQuickFilters"));
    assert!(activity_views.contains("renderActivityTagFilters"));
    assert!(activity_views.contains("activity-filter-chip"));
    assert!(activity_views.contains("activity-filter-count"));
    assert!(activity_views.contains("is-active"));
    assert!(activity_views.contains("aria-pressed"));
    for status in ["open", "not_started", "ended"] {
        assert!(activity_views.contains(&format!("[\"{status}\",")));
    }
    assert!(!activity_views.contains("cell.disabled = true"));
    assert!(activity_views.contains("首页滚动图"));
    assert!(activity_views.contains("限时任务"));

    let stylesheet = request(service.port(), "GET", "/manage/style.css");
    assert!(stylesheet.starts_with("HTTP/1.1 200 OK"));
    assert!(stylesheet.contains("Content-Type: text/css; charset=utf-8"));
    assert!(stylesheet.contains(".activity-calendar"));
    assert!(stylesheet.contains(".activity-banner-frame"));
    assert!(stylesheet.contains(".http-observation-history"));
    assert!(stylesheet.contains(".shop-stock-panel"));
    let banner_frame_rule = stylesheet
        .split(".activity-banner-frame {")
        .nth(1)
        .and_then(|rules| rules.split('}').next())
        .expect("activity banner frame rule exists");
    assert!(banner_frame_rule.contains("width: 100%"));
    assert!(banner_frame_rule.contains("max-width: 100%"));
    let banner_image_rule = stylesheet
        .split(".activity-banner-frame img {")
        .nth(1)
        .and_then(|rules| rules.split('}').next())
        .expect("activity banner image rule exists");
    assert!(banner_image_rule.contains("max-width: 100%"));
    assert!(banner_image_rule.contains("object-fit: contain"));

    let rejected = request(service.port(), "POST", "/manage/");
    assert!(rejected.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(rejected.contains("Allow: GET"));

    let direct = request(service.port(), "GET", "/v1/server-profiles");
    assert!(direct.starts_with("HTTP/1.1 200 OK"));
    service.stop().expect("service stops cleanly");
}
// //// /提供打开即用的本机管理页面 ////
