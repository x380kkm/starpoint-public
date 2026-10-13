// audience: internal
// # personal-service-cn-mission-battle-conditions-api-tests
//
// 实际战斗接口覆盖觉醒限时任务与普通任务奖励, 重放和重启保留同一份进度.

use super::*;

fn progress(service:&PersonalService, viewer:i64, category:i64, mission:i64)->i64 {
    let response=decode_response::<Value>(&send(service,"/api/index.php/mission/get_mission_progress",json!({
        "viewer_id":viewer,"api_count":1,"category_list":[{"category":category}],
    }))).data;
    response["mission_progress_list"].as_array().unwrap().iter().find(|entry|entry["mission_id"]==mission).unwrap()["progress_value"].as_i64().unwrap()
}

// //// 从真实专家关卡结算发奖并隔离结算重放 [@x380kkm 2026-09-22] ////
#[test]
fn expert_timed_awake_reward_survives_replay_and_restart() {
    let root=TempDir::new().unwrap();
    let service=PersonalService::start(root.path(),0).unwrap();
    let viewer=signup(&service,922116);
    set_time(&service,"2026-09-22T12:00:00.000Z");
    open_activity(&service,"expert-single:1");
    service.stop().unwrap();
    update_player(root.path(),|player|{
        player["user_party_group_list"]["1"]["list"]["1"]["character_ids"]=json!([231001,null,null]);
        player["user_character_list"]["231001"]=player["user_character_list"]["1"].clone();
        player["user_info"]["stamina"]=json!(9999);
    });
    let service=PersonalService::start(root.path(),0).unwrap();
    assert_eq!(progress(&service,viewer,9,2310013),0);
    assert_eq!(progress(&service,viewer,1,42),0);
    let before=load(&service,viewer)["item_list"]["11"].as_i64().unwrap_or_default();
    decode_response::<Value>(&send(&service,"/api/index.php/single_battle_quest/start",json!({
        "viewer_id":viewer,"category":21,"quest_id":1006,"play_id":"expert-timed-mission","party_id":1,
        "use_boost_point":false,"use_boss_boost_point":false,"is_auto_start_mode":false,
    })));
    let request=json!({
        "viewer_id":viewer,"category":21,"quest_id":1006,"play_id":"expert-timed-mission",
        "elapsed_time_ms":90000,"score":1000,"add_mana":0,"is_accomplished":true,
        "statistics":{"clear_phase":1,"max_power":6000,"max_combo_count":600,"zones":[{"damage_deal_max":1000000}],
            "party":{"characters":[{"id":231001},null,null],"unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}},
    });
    let response=decode_response::<Value>(&send(&service,"/api/index.php/single_battle_quest/finish",request.clone())).data;
    assert!(response["mission_info"].as_array().unwrap().iter().any(|entry|entry["mission_category_id"]==9&&entry["mission_id"]==2310013));
    assert_eq!(progress(&service,viewer,9,2310013),1);
    assert_eq!(progress(&service,viewer,1,42),0);
    assert_eq!(progress(&service,viewer,1,30),6000);
    assert_eq!(progress(&service,viewer,9,1210013),0);
    let rewarded=load(&service,viewer);
    assert!(rewarded["item_list"]["11"].as_i64().unwrap()>=before+3);
    let replay=decode_response::<Value>(&send(&service,"/api/index.php/single_battle_quest/finish",request)).data;
    assert_eq!(replay,response);
    assert_eq!(load(&service,viewer)["item_list"],rewarded["item_list"]);
    service.stop().unwrap();
    let service=PersonalService::start(root.path(),0).unwrap();
    assert_eq!(progress(&service,viewer,9,2310013),1);
    assert_eq!(load(&service,viewer)["item_list"],rewarded["item_list"]);
    service.stop().unwrap();
}
// //// /从真实专家关卡结算发奖并隔离结算重放 ////

// //// 竞速未通关时只完成实际到达阶段并保持重放幂等 [@x380kkm 2026-09-22] ////
#[test]
fn ranking_partial_finish_grants_only_completed_phase_missions() {
    let root=TempDir::new().unwrap();
    let service=PersonalService::start(root.path(),0).unwrap();
    let viewer=signup(&service,922117);
    set_time(&service,"2022-12-16T12:00:00.000Z");
    open_activity(&service,"ranking:3");
    service.stop().unwrap();
    update_player(root.path(),|player| {
        player["user_info"]["stamina"]=json!(9999);
        player["user_info"]["stamina_heal_time"]=json!(1671192000);
    });
    let service=PersonalService::start(root.path(),0).unwrap();
    let original_clears=progress(&service,viewer,1,6);
    for (play,complete,phases) in [("rank-zero",false,0),("rank-partial",false,2),("rank-complete",true,1)] {
        decode_response::<Value>(&send(&service,"/api/index.php/single_battle_quest/start",json!({
            "viewer_id":viewer,"category":11,"quest_id":3001,"play_id":play,"party_id":1,
            "use_boost_point":false,"use_boss_boost_point":false,"is_auto_start_mode":false,
        })));
        let before=load(&service,viewer);
        let request=json!({
            "viewer_id":viewer,"category":11,"quest_id":3001,"play_id":play,"elapsed_time_ms":90000,
            "score":1000,"add_mana":0,"is_accomplished":complete,
            "statistics":{"clear_phase":phases,"zones":[],"party":{"characters":[{"id":1}],"unison_characters":[],"equipments":[],"ability_soul_ids":[]}},
        });
        let response=decode_response::<Value>(&send(&service,"/api/index.php/single_battle_quest/finish",request.clone())).data;
        let after=load(&service,viewer);
        for (id,phase) in [(200841,1),(200842,2),(200843,3),(200844,4)] {
            let expected=if complete {if phase<=2 {2} else {1}} else {i64::from(phases>=phase)};
            assert_eq!(progress(&service,viewer,3,id),expected,"{play}:{id}");
        }
        if play=="rank-zero" {
            assert_eq!(after["user_info"]["free_vmoney"],before["user_info"]["free_vmoney"]);
        }
        if play=="rank-partial" {
            assert!(after["user_info"]["free_vmoney"].as_i64().unwrap()>=before["user_info"]["free_vmoney"].as_i64().unwrap()+150);
            assert!(after["item_list"]["49100"].as_i64().unwrap()>=before["item_list"]["49100"].as_i64().unwrap_or_default()+5);
            assert_eq!(progress(&service,viewer,1,6),original_clears);
        }
        assert_eq!(decode_response::<Value>(&send(&service,"/api/index.php/single_battle_quest/finish",request)).data,response);
        assert_eq!(load(&service,viewer)["item_list"],after["item_list"]);
    }
    service.stop().unwrap();
    let service=PersonalService::start(root.path(),0).unwrap();
    assert_eq!(progress(&service,viewer,3,200841),2);
    assert_eq!(progress(&service,viewer,3,200844),1);
    service.stop().unwrap();
}
// //// /竞速未通关时只完成实际到达阶段并保持重放幂等 ////
