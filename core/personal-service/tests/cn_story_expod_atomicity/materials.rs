// audience: internal
// # personal-service-cn-character-material-tests
// 角色突破素材遵循主表星级范围, 成功请求的重放保留原消耗和任务结果.

use super::*;

// //// 按五种角色星级核对三种破星结晶的适用范围 [@x380kkm 2026-09-23] ////
#[test]
fn applies_uncapping_crystals_only_to_supported_character_rarities() {
    let (root, service, viewer) = setup();
    let template = player_state(root.path()).snapshot["user_character_list"]["1"].clone();
    for (character_id, rarity) in [
        (512_001, 1),
        (412_004, 2),
        (311_001, 3),
        (1, 4),
        (111_001, 5),
    ] {
        for item_id in 10_001..=10_003 {
            update_player(root.path(), |player| {
                let mut character = template.clone();
                character["over_limit_step"] = json!(0);
                character["stack"] = json!(3);
                player["user_character_list"][character_id.to_string()] = character;
                player["item_list"][item_id.to_string()] = json!(2);
            });
            let before = player_state(root.path());
            let body = json!({
                "viewer_id": viewer, "character_id": character_id,
                "over_limit_count": 2, "use_stack": false, "item_id": item_id,
                "api_count": rarity * 100 + item_id
            });
            let response = request(&service, "character/over_limit", body.clone());
            let allowed = match item_id {
                10_001 => rarity <= 3,
                10_002 => rarity <= 4,
                10_003 => rarity == 5,
                _ => unreachable!(),
            };
            if !allowed {
                assert!(
                    response.starts_with("HTTP/1.1 400"),
                    "{character_id}/{item_id}: {response}"
                );
                assert!(response.ends_with("{\"error\":\"invalid_uncapping_item\"}"));
                assert!(player_state(root.path()) == before);
                continue;
            }
            let first = decode_response::<Value>(&response).data;
            let after = player_state(root.path());
            assert_eq!(after.snapshot["item_list"][item_id.to_string()], 0);
            assert_eq!(
                after.snapshot["user_character_list"][character_id.to_string()]["over_limit_step"],
                2
            );
            assert_eq!(
                after.snapshot["user_character_list"][character_id.to_string()]["stack"],
                3
            );
            assert_eq!(
                decode_response::<Value>(&request(&service, "character/over_limit", body)).data,
                first
            );
            assert!(player_state(root.path()) == after);
        }
    }
    service.stop().unwrap();
}
// //// /按五种角色星级核对三种破星结晶的适用范围 ////
