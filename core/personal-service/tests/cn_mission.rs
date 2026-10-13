// audience: internal
// # personal-service-cn-mission-tests
//
// 该文件验证 CN mission master 查询、响应契约、模式匹配更新和重启持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct MissionCategory {
    category: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    character_id: Option<i64>,
}

#[derive(Serialize)]
struct GetMissionProgressRequest {
    viewer_id: i64,
    api_count: i64,
    category_list: Vec<MissionCategory>,
}

#[derive(Serialize)]
struct MissionParameter<'a> {
    progress_value: i64,
    mission_pattern: &'a str,
}

#[derive(Serialize)]
struct UpdateMissionProgressRequest<'a> {
    viewer_id: i64,
    api_count: i64,
    mission_param_list: Vec<MissionParameter<'a>>,
}

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn create_mail(service: &PersonalService, viewer_id: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Mission mail\",\"body\":\"Mail state\",\"sender\":\"Admin\",\"rewards\":{{\"freeVmoney\":1}}}}"
    );
    let response = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"));
}

fn get_progress(
    service: &PersonalService,
    viewer_id: i64,
    categories: Vec<MissionCategory>,
) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mission/get_mission_progress",
        &encode_request(&GetMissionProgressRequest {
            viewer_id,
            api_count: 1,
            category_list: categories,
        }),
    ))
    .data
}

fn mission<'a>(data: &'a Value, category: i64, mission_id: i64) -> &'a Value {
    data["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mission| {
            mission["mission_category"] == category && mission["mission_id"] == mission_id
        })
        .unwrap()
}

// //// 验证多类别 master 查询和角色觉醒筛选 [@x380kkm 2026-08-22] ////
#[test]
fn returns_master_categories_and_filtered_character_awake_missions() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 49);

    let categories = get_progress(
        &service,
        viewer_id,
        vec![
            MissionCategory {
                category: 1,
                event_id: None,
                character_id: None,
            },
            MissionCategory {
                category: 10,
                event_id: None,
                character_id: None,
            },
        ],
    );
    let progress = categories["mission_progress_list"].as_array().unwrap();
    assert_eq!(
        progress
            .iter()
            .filter(|mission| mission["mission_category"] == 1)
            .count(),
        120
    );
    assert_eq!(
        progress
            .iter()
            .filter(|mission| mission["mission_category"] == 10)
            .count(),
        2
    );
    assert!(progress
        .iter()
        .all(|mission| mission["stage"].as_i64().is_some_and(|stage| stage > 0)));

    let awake = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 9,
            event_id: None,
            character_id: Some(1),
        }],
    );
    let awake_ids = awake["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mission| mission["mission_id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(awake_ids, vec![11, 12, 13, 14]);

    let awake_pair = get_progress(
        &service,
        viewer_id,
        [1, 111001]
            .into_iter()
            .map(|character_id| MissionCategory {
                category: 9,
                event_id: None,
                character_id: Some(character_id),
            })
            .collect(),
    );
    let awake_pair_ids = awake_pair["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mission| mission["mission_id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        awake_pair_ids,
        vec![11, 12, 13, 14, 1110011, 1110012, 1110013, 1110014]
    );

    let repeated = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 1,
            event_id: None,
            character_id: None,
        }],
    );
    let repeated_again = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 1,
            event_id: None,
            character_id: None,
        }],
    );
    assert_eq!(repeated_again, repeated);
    service.stop().expect("service stops cleanly");
}
// //// /验证多类别 master 查询和角色觉醒筛选 ////

// //// 验证收集活动任务按活动标识隔离 [@x380kkm 2026-09-21] ////
#[test]
fn returns_only_requested_collect_item_event_missions() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 52);
    for (event_id, expected_ids) in [
        (1, (1500..=1573).collect::<Vec<_>>()),
        (10003, (1760..=1778).collect()),
        (10004, (1779..=1800).collect()),
    ] {
        let response = get_progress(
            &service,
            viewer_id,
            vec![MissionCategory {
                category: 4,
                event_id: Some(event_id),
                character_id: None,
            }],
        );
        let actual_ids = response["mission_progress_list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mission| mission["mission_id"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual_ids, expected_ids, "collect item event {event_id}");
    }
    for event_id in [None, Some(0)] {
        let response = cn_support::send_request(
            service.port(),
            "/api/index.php/mission/get_mission_progress",
            &encode_request(&GetMissionProgressRequest {
                viewer_id,
                api_count: 1,
                category_list: vec![MissionCategory {
                    category: 4,
                    event_id,
                    character_id: None,
                }],
            }),
        );
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证收集活动任务按活动标识隔离 ////

// //// 验证客户端界面计数与服务端动作校验并保存奖励 [@x380kkm 2026-09-22] ////
#[test]
fn accumulates_client_counts_and_settles_visible_rewards_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 50);
    let load = |service: &PersonalService, viewer_id| decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/load", &encode_request(&serde_json::json!({"viewer_id":viewer_id,"keychain":viewer_id})),
    )).data;
    get_progress(&service, viewer_id, vec![MissionCategory {category:1,event_id:None,character_id:None}]);
    let initial_stones = load(&service, viewer_id)["user_info"]["free_vmoney"].as_i64().unwrap();
    let mut latest = Value::Null;
    for _ in 0..5 {
        latest = decode_response::<Value>(&cn_support::send_request(
            service.port(), "/api/index.php/mission/update_mission_progress",
            &encode_request(&UpdateMissionProgressRequest { viewer_id, api_count: 2, mission_param_list: vec![
                MissionParameter { progress_value: 1, mission_pattern: "home_tap_town_character_count" },
            ] }),
        )).data;
    }
    assert!(latest["degree_list"].as_array().unwrap().iter().any(|degree| degree["degree_id"] == 49000));
    let progress = get_progress(&service, viewer_id, vec![MissionCategory {category:5,event_id:None,character_id:None}]);
    assert_eq!(mission(&progress, 5, 49000)["progress_value"], 5);
    for pattern in ["max_combo_achievement", "total_gacha_character_count", "mana_addition_count", "total_used_mana_count", "treasure_shop_bought_item_count"] {
        let update = UpdateMissionProgressRequest { viewer_id, api_count: 3, mission_param_list: vec![
            MissionParameter { progress_value: 1, mission_pattern: "home_tap_town_character_count" },
            MissionParameter { progress_value: 10_000, mission_pattern: pattern },
        ] };
        let response = cn_support::send_request(service.port(), "/api/index.php/mission/update_mission_progress", &encode_request(&update));
        assert!(response.contains("invalid_mission_parameter"), "{response}");
    }
    let allowed = UpdateMissionProgressRequest { viewer_id, api_count: 4, mission_param_list: [
        "character_detail_zoom_illust_for_1min_count", "character_detail_play_dot_sp_motion_count", "home_change_voice_count", "twitter_check",
    ].into_iter().map(|mission_pattern| MissionParameter { progress_value: 1, mission_pattern }).collect() };
    decode_response::<Value>(&cn_support::send_request(service.port(), "/api/index.php/mission/update_mission_progress", &encode_request(&allowed)));
    let regular = get_progress(&service, viewer_id, vec![MissionCategory {category:1,event_id:None,character_id:None}]);
    assert_eq!(mission(&regular, 1, 1)["progress_value"], 0);
    assert_eq!(load(&service, viewer_id)["user_info"]["free_vmoney"], initial_stones);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 50);
    assert_eq!(load(&service, viewer_id)["user_info"]["free_vmoney"], initial_stones);
    let persisted = get_progress(&service, viewer_id, vec![MissionCategory {category:5,event_id:None,character_id:None}]);
    assert_eq!(mission(&persisted, 5, 49000)["progress_value"], 5);
    service.stop().unwrap();
}
// //// /验证客户端界面计数与服务端动作校验并保存奖励 ////

// //// 验证任务查询和更新返回当前邮件状态 [@x380kkm 2026-08-25] ////
#[test]
fn returns_current_mail_arrival_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 51);
    create_mail(&service, viewer_id);

    let progress = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 1,
            event_id: None,
            character_id: None,
        }],
    );
    assert_eq!(progress["mail_arrived"], true);

    let updated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mission/update_mission_progress",
        &encode_request(&UpdateMissionProgressRequest {
            viewer_id,
            api_count: 2,
            mission_param_list: vec![MissionParameter {
                progress_value: 1,
                mission_pattern: "home_tap_town_character_count",
            }],
        }),
    ));
    assert_eq!(updated.data["mail_arrived"], true);
    service.stop().expect("service stops cleanly");
}
// //// /验证任务查询和更新返回当前邮件状态 ////
