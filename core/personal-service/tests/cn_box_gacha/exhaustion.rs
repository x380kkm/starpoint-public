// audience: internal
// # personal-service-cn-box-gacha-exhaustion-tests
//
// 该文件通过实际箱池目录验证末抽角色发奖, 整箱抽取和循环箱重置.

use super::*;

// //// 验证全部角色箱池末抽和重复角色素材持久化 [@x380kkm 2026-09-27] ////
#[test]
fn grants_each_character_reward_when_it_is_the_last_capsule() {
    for (pool, reward_id, character_id, duplicate_item_id) in [
        (1, "1000103001", 263002, 14014),
        (2, "1000203001", 233001, 14008),
        (3, "1000303001", 223001, 14005),
    ] {
        let root = TempDir::new().expect("temporary service directory is created");
        let cdn_root = TempDir::new().expect("CN asset root is created");
        let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
            .expect("service starts");
        let signup = decode_response::<SignupData>(&cn_support::send_request(
            service.port(),
            "/api/index.php/tool/signup",
            &encode_request(&SignupRequest { device_id: 71 }),
        ));
        let viewer_id = signup.data_headers.viewer_id;
        let currency = (30100 + pool).to_string();
        let character = character_id.to_string();
        let duplicate_item = duplicate_item_id.to_string();
        for repetition in 0..2 {
            set_remaining_rewards(&root, pool, 3, &[reward_id]);
            update_player_snapshot(&root, |player_data| {
                player_data["item_list"][&currency] = json!(10);
            });
            let before = read_player_snapshot(&root);
            let request = encode_request(&BoxExecRequest {
                viewer_id,
                box_gacha_id: pool,
                box_id: 3,
                number: 1,
                stop_on_featured_rewards: false,
                api_count: Some(730 + repetition),
            });
            let response =
                cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request);
            assert!(
                response.starts_with("HTTP/1.1 200 OK"),
                "pool {pool}: {response}"
            );
            let draw = decode_response::<Value>(&response);
            let saved = read_player_snapshot(&root);
            assert_eq!(
                draw.data["drawn_reward_list"],
                json!([{
                    "reward_id":reward_id.parse::<i64>().unwrap(), "number":1
                }])
            );
            assert_eq!(
                saved["box_gacha_list"][pool.to_string()][0]["remaining_number"],
                0
            );
            assert_eq!(saved["item_list"][&currency], 0);
            assert_eq!(draw.data["character_list"].as_array().unwrap().len(), 1);
            assert_eq!(draw.data["character_list"][0]["character_id"], character_id);
            assert_eq!(
                saved["user_character_list"][&character]["stack"],
                repetition
            );
            if repetition == 0 {
                assert_eq!(draw.data["joined_character_id_list"], json!([character_id]));
            } else {
                assert_eq!(draw.data["joined_character_id_list"], json!([]));
                assert_eq!(draw.data["character_list"][0]["stack"], 1);
                let previous = before["item_list"][&duplicate_item]
                    .as_i64()
                    .unwrap_or_default();
                assert_eq!(saved["item_list"][&duplicate_item], previous + 1);
                assert_eq!(draw.data["item_list"][&duplicate_item], previous + 1);
            }
            assert_eq!(
                cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request,),
                response
            );
            assert_eq!(read_player_snapshot(&root), saved);
        }
        service.stop().expect("service stops cleanly");
    }
}
// //// /验证全部角色箱池末抽和重复角色素材持久化 ////

// //// 验证客户端批量抽数清空终箱并重置循环箱 [@x380kkm 2026-09-27] ////
#[test]
fn exhausts_large_boxes_and_draws_again_after_reset() {
    let rewards: Value = serde_json::from_str(include_str!("../../../../assets/box_reward.json"))
        .expect("box reward catalog is read");
    for pool in [1, 2] {
        let root = TempDir::new().expect("temporary service directory is created");
        let cdn_root = TempDir::new().expect("CN asset root is created");
        let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
            .expect("service starts");
        let signup = decode_response::<SignupData>(&cn_support::send_request(
            service.port(),
            "/api/index.php/tool/signup",
            &encode_request(&SignupRequest { device_id: 71 }),
        ));
        let viewer_id = signup.data_headers.viewer_id;
        let pool_key = pool.to_string();
        let currency = (30100 + pool).to_string();
        let box_rewards = rewards[&pool_key]["4"].as_object().unwrap();
        let total: i64 = box_rewards
            .values()
            .map(|reward| reward["available"].as_i64().unwrap())
            .sum();
        update_player_snapshot(&root, |player_data| {
            player_data["item_list"][&currency] = json!(10 * (total + 1));
        });
        let request = encode_request(&BoxExecRequest {
            viewer_id,
            box_gacha_id: pool,
            box_id: 4,
            number: 9999,
            stop_on_featured_rewards: pool == 2,
            api_count: Some(800),
        });
        let response =
            cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "pool {pool}: {response}"
        );
        let draw = decode_response::<Value>(&response);
        let saved = read_player_snapshot(&root);
        assert_eq!(saved["item_list"][&currency], 10);
        assert_eq!(draw.data["item_list"][&currency], 10);
        let drawn = draw.data["drawn_reward_list"].as_array().unwrap();
        assert_eq!(drawn.len(), box_rewards.len());
        for reward in drawn {
            let id = reward["reward_id"].as_i64().unwrap().to_string();
            assert_eq!(reward["number"], box_rewards[&id]["available"]);
            assert_eq!(
                saved["box_gacha_drawn_rewards"][&pool_key]["4"][&id],
                reward["number"]
            );
        }
        let listing = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/box_gacha/get_box_list",
            &encode_request(&BoxListRequest {
                viewer_id,
                box_gacha_id: pool,
            }),
        ));
        let empty_box = listing.data["all_box_info"]
            .as_array()
            .unwrap()
            .iter()
            .find(|status| status["box_id"] == 4)
            .unwrap();
        assert_eq!(empty_box["is_closed"], true);
        assert_eq!(
            empty_box["all_drawn_reward_list"],
            draw.data["drawn_reward_list"]
        );
        assert_eq!(
            cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request,),
            response
        );
        assert_eq!(read_player_snapshot(&root), saved);
        if pool == 2 {
            let reset = decode_response::<Value>(&cn_support::send_request(
                service.port(),
                "/api/index.php/box_gacha/reset",
                &encode_request(&BoxResetRequest {
                    viewer_id,
                    box_gacha_id: pool,
                    box_id: 4,
                }),
            ));
            let reset_box = reset.data["all_box_info"]
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["box_id"] == 4)
                .unwrap();
            assert_eq!(reset_box["is_closed"], false);
            assert_eq!(reset_box["reset_times"], 1);
            assert_eq!(reset_box["all_drawn_reward_list"], json!([]));
            let next = decode_response::<Value>(&cn_support::send_request(
                service.port(),
                "/api/index.php/box_gacha/exec",
                &encode_request(&BoxExecRequest {
                    viewer_id,
                    box_gacha_id: pool,
                    box_id: 4,
                    number: 1,
                    stop_on_featured_rewards: false,
                    api_count: Some(801),
                }),
            ));
            assert_eq!(next.data["drawn_reward_list"].as_array().unwrap().len(), 1);
            assert_eq!(next.data["drawn_reward_list"][0]["number"], 1);
            assert_eq!(next.data["item_list"][&currency], 0);
            let saved = read_player_snapshot(&root);
            let status = saved["box_gacha_list"][&pool_key]
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["box_id"] == 4)
                .unwrap();
            assert_eq!(status["remaining_number"], total - 1);
        }
        service.stop().expect("service stops cleanly");
    }
}
// //// /验证客户端批量抽数清空终箱并重置循环箱 ////

