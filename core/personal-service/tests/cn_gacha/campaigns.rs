// audience: internal
// # personal-service-cn-gacha-campaign-tests
// 免费活动通过 HTTP 入口选择, 次数与重放记录来自隔离数据库.

use super::*;

// //// 设置免费活动的管理开放状态 [@x380kkm 2026-09-28] ////
pub(super) fn set_campaign_enabled(service: &PersonalService, campaign_id: i64, enabled: bool) {
    let bearer = format!("Bearer {}", service.management_token());
    let body =
        json!({"enabled": enabled, "start_at_ms": 0, "end_at_ms": 253402300799000_i64}).to_string();
    let response = request_with_headers(
        service.port(),
        "PUT",
        &format!("/v1/activities/calendar/gacha-campaign%3A{campaign_id}"),
        "application/json",
        &[("Authorization", &bearer)],
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

fn load(service: &PersonalService, viewer_id: i64) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ))
    .data
}

fn draw(
    service: &PersonalService,
    viewer_id: i64,
    api_count: i64,
    gacha_id: i64,
    draw_type: i64,
) -> String {
    cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count,
            payment_type: 4,
            number_of_exec: 1,
            viewer_id,
            gacha_id,
            r#type: draw_type,
        }),
    )
}

// //// 保留同池连续活动并按类型和管理窗口兑换 [@x380kkm 2026-09-28] ////
#[test]
fn preserves_overlapping_campaign_relations_and_exhausted_selection() {
    let directory = TempDir::new().unwrap();
    let service = PersonalService::start(directory.path(), 0).unwrap();
    set_virtual_time(&service, "2023-08-18T04:00:00.000Z");
    for id in [1577, 1579] {
        open_gacha(&service, id);
    }
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 9701 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let initial = load(&service, viewer_id);
    for id in [1577, 1579] {
        let entries = initial["gacha_campaign_list"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["gacha_id"] == id)
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["campaign_id"], 1011);
        assert_eq!(entries[0]["count"], 1);
    }
    let before = replay::stored_state(directory.path());
    assert!(
        draw(&service, viewer_id, 1, 1577, 8).ends_with("{\"error\":\"gacha_campaign_not_found\"}")
    );
    assert_eq!(replay::stored_state(directory.path()), before);
    let first = draw(&service, viewer_id, 2, 1577, 11);
    let result = decode_response::<Value>(&first).data;
    assert_eq!(result["draw"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["gacha_campaign_list"][0],
        json!({"gacha_id": 1577, "campaign_id": 1011, "count": 0})
    );
    assert_eq!(draw(&service, viewer_id, 2, 1577, 11), first);
    set_campaign_enabled(&service, 1012, true);
    let overlap = load(&service, viewer_id);
    let entries = overlap["gacha_campaign_list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["gacha_id"] == 1577)
        .collect::<Vec<_>>();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["campaign_id"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![1011, 1_001_577]
    );
    assert!(draw(&service, viewer_id, 3, 1577, 11)
        .ends_with("{\"error\":\"gacha_campaign_already_used\"}"));
    let other_pool = decode_response::<Value>(&draw(&service, viewer_id, 4, 1579, 11)).data;
    assert_eq!(other_pool["gacha_campaign_list"][0]["campaign_id"], 1011);
    let second = decode_response::<Value>(&draw(&service, viewer_id, 5, 1577, 11)).data;
    assert_eq!(second["gacha_campaign_list"][0]["campaign_id"], 1_001_577);
    let stored = replay::stored_state(directory.path()).0;
    for id in [1011, 1012] {
        assert!(stored["gacha_campaign_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["gacha_id"] == 1577 && entry["campaign_id"] == id && entry["count"] == 0
            }));
    }
    set_campaign_enabled(&service, 1011, false);
    set_campaign_enabled(&service, 1012, false);
    assert!(draw(&service, viewer_id, 6, 1577, 11)
        .ends_with("{\"error\":\"gacha_campaign_not_found\"}"));
    set_campaign_enabled(&service, 1012, true);
    service.stop().unwrap();
    let service = PersonalService::start(directory.path(), 0).unwrap();
    assert_eq!(draw(&service, viewer_id, 2, 1577, 11), first);
    set_campaign_enabled(&service, 1011, true);
    set_virtual_time(&service, "2023-08-19T04:00:00.000Z");
    let next_day = load(&service, viewer_id);
    let available = next_day["gacha_campaign_list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["gacha_id"] == 1577)
        .collect::<Vec<_>>();
    assert_eq!(available.len(), 2);
    assert!(available.iter().all(|entry| entry["count"] == 1));
    let next = decode_response::<Value>(&draw(&service, viewer_id, 7, 1577, 11)).data;
    assert_eq!(next["gacha_campaign_list"][0]["campaign_id"], 1011);
    service.stop().unwrap();
}

// //// 临时池分别投影同池不同活动的客户端入口 [@x380kkm 2026-09-28] ////
#[test]
fn temporary_pool_preserves_every_campaign_alias_and_canonical_count() {
    let directory = TempDir::new().unwrap();
    let service = PersonalService::start(directory.path(), 0).unwrap();
    set_virtual_time(&service, "2030-01-01T04:00:00.000Z");
    open_gacha(&service, 49);
    temporarily_open_gacha(&service, 49);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 9702 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let initial = load(&service, viewer_id);
    let aliases = initial["gacha_campaign_list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["gacha_id"] == 1_000_049)
        .map(|entry| entry["campaign_id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(aliases, vec![4_000_049, 1_000_049]);
    for (api_count, draw_type, campaign_id, pulls) in [(1, 11, 4_000_049, 1), (2, 8, 1_000_049, 10)]
    {
        let result =
            decode_response::<Value>(&draw(&service, viewer_id, api_count, 1_000_049, draw_type))
                .data;
        assert_eq!(result["draw"].as_array().unwrap().len(), pulls);
        assert_eq!(result["gacha_campaign_list"][0]["campaign_id"], campaign_id);
    }
    let stored = replay::stored_state(directory.path()).0;
    let campaigns = stored["gacha_campaign_list"].as_array().unwrap();
    for campaign_id in [2, 3] {
        assert!(campaigns.iter().any(|entry| entry["gacha_id"] == 49
            && entry["campaign_id"] == campaign_id
            && entry["count"] == 0));
    }
    assert!(campaigns.iter().all(|entry| entry["gacha_id"] != 1_000_049));
    open_gacha(&service, 80000);
    set_campaign_enabled(&service, 12, true);
    let permanent_pool = load(&service, viewer_id);
    assert!(permanent_pool["gacha_campaign_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["gacha_id"] == 80000 && entry["campaign_id"] == 14_080_000));
    let permanent_draw = decode_response::<Value>(&draw(&service, viewer_id, 3, 80000, 8)).data;
    assert_eq!(
        permanent_draw["gacha_campaign_list"][0]["campaign_id"],
        14_080_000
    );
    let stored = replay::stored_state(directory.path()).0;
    assert!(stored["gacha_campaign_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["gacha_id"] == 80000
            && entry["campaign_id"] == 12
            && entry["count"] == 0));
    service.stop().unwrap();
}
