// audience: internal
// # activity-projection-files
//
// 该模块原子写入活动投影文件, 并把生成的活动 master 原始字节绑定到运行时.

use crate::PersonalServiceError;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// //// 原子写入活动投影文件 [@x380kkm 2026-08-29] ////
pub(super) fn atomic_write(path: &Path, data: &[u8]) -> Result<(), PersonalServiceError> {
    let parent = path
        .parent()
        .ok_or_else(|| PersonalServiceError::new("CN activity projection path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to create CN activity projection directory: {error}"
        ))
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = parent.join(format!(
        ".{}.tmp-{}-{nonce}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        fs::write(&temporary_path, data).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to write CN activity projection file: {error}"
            ))
        })?;
        match fs::rename(&temporary_path, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                fs::remove_file(path).map_err(|remove_error| {
                    PersonalServiceError::new(format!(
                        "failed to replace CN activity projection file: {remove_error}"
                    ))
                })?;
                fs::rename(&temporary_path, path).map_err(|rename_error| {
                    PersonalServiceError::new(format!(
                        "failed to replace CN activity projection file: {rename_error}"
                    ))
                })
            }
            Err(error) => Err(PersonalServiceError::new(format!(
                "failed to commit CN activity projection file: {error}"
            ))),
        }
    })();
    let _ = fs::remove_file(&temporary_path);
    result
}

pub(super) fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), PersonalServiceError> {
    let mut data = serde_json::to_vec_pretty(value).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode CN activity projection JSON: {error}"
        ))
    })?;
    data.push(b'\n');
    atomic_write(path, &data)
}
// //// /原子写入活动投影文件 ////

// //// 绑定生成的 CN 活动 master 原始种子 [@x380kkm 2026-08-29] ////
pub(super) fn master_seed(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "event_list" => include_bytes!("../../assets/cn-activity-masters/event_list.orderedmap"),
        "advent_event" => {
            include_bytes!("../../assets/cn-activity-masters/advent_event.orderedmap")
        }
        "ranking_event" => {
            include_bytes!("../../assets/cn-activity-masters/ranking_event.orderedmap")
        }
        "story_event" => include_bytes!("../../assets/cn-activity-masters/story_event.orderedmap"),
        "daily_week_event" => {
            include_bytes!("../../assets/cn-activity-masters/daily_week_event.orderedmap")
        }
        "challenge_dungeon_event" => {
            include_bytes!("../../assets/cn-activity-masters/challenge_dungeon_event.orderedmap")
        }
        "daily_exp_mana_event" => {
            include_bytes!("../../assets/cn-activity-masters/daily_exp_mana_event.orderedmap")
        }
        "world_story_event" => {
            include_bytes!("../../assets/cn-activity-masters/world_story_event.orderedmap")
        }
        "tower_dungeon_event" => {
            include_bytes!("../../assets/cn-activity-masters/tower_dungeon_event.orderedmap")
        }
        "expert_single_event" => {
            include_bytes!("../../assets/cn-activity-masters/expert_single_event.orderedmap")
        }
        "carnival_event" => {
            include_bytes!("../../assets/cn-activity-masters/carnival_event.orderedmap")
        }
        "raid_event" => include_bytes!("../../assets/cn-activity-masters/raid_event.orderedmap"),
        "rush_event" => include_bytes!("../../assets/cn-activity-masters/rush_event.orderedmap"),
        "solo_time_attack_event" => {
            include_bytes!("../../assets/cn-activity-masters/solo_time_attack_event.orderedmap")
        }
        "hard_multi_event" => {
            include_bytes!("../../assets/cn-activity-masters/hard_multi_event.orderedmap")
        }
        "score_attack_event" => {
            include_bytes!("../../assets/cn-activity-masters/score_attack_event.orderedmap")
        }
        "collect_item_event" => {
            include_bytes!("../../assets/cn-activity-masters/collect_item_event.orderedmap")
        }
        "active_mission_event" => {
            include_bytes!("../../assets/cn-activity-masters/active_mission_event.orderedmap")
        }
        "event_shop_select_item_campaign" => include_bytes!(
            "../../assets/cn-activity-masters/event_shop_select_item_campaign.orderedmap"
        ),
        "box_gacha" => {
            include_bytes!("../../assets/cn-activity-masters/box_gacha.orderedmap")
        }
        "pass_card_event" => {
            include_bytes!("../../assets/cn-activity-masters/pass_card_event.orderedmap")
        }
        "advent_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/advent_event_quest.orderedmap")
        }
        "story_event_single_quest" => {
            include_bytes!("../../assets/cn-activity-masters/story_event_single_quest.orderedmap")
        }
        "daily_week_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/daily_week_event_quest.orderedmap")
        }
        "daily_exp_mana_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/daily_exp_mana_event_quest.orderedmap")
        }
        "challenge_dungeon_event_quest" => {
            include_bytes!(
                "../../assets/cn-activity-masters/challenge_dungeon_event_quest.orderedmap"
            )
        }
        "world_story_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/world_story_event_quest.orderedmap")
        }
        "world_story_event_boss_battle_quest" => include_bytes!(
            "../../assets/cn-activity-masters/world_story_event_boss_battle_quest.orderedmap"
        ),
        "tower_dungeon_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/tower_dungeon_event_quest.orderedmap")
        }
        "expert_single_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/expert_single_event_quest.orderedmap")
        }
        "carnival_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/carnival_event_quest.orderedmap")
        }
        "raid_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/raid_event_quest.orderedmap")
        }
        "ranking_event_single_quest" => {
            include_bytes!("../../assets/cn-activity-masters/ranking_event_single_quest.orderedmap")
        }
        "rush_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/rush_event_quest.orderedmap")
        }
        "solo_time_attack_event_quest" => {
            include_bytes!(
                "../../assets/cn-activity-masters/solo_time_attack_event_quest.orderedmap"
            )
        }
        "hard_multi_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/hard_multi_event_quest.orderedmap")
        }
        "score_attack_event_quest" => {
            include_bytes!("../../assets/cn-activity-masters/score_attack_event_quest.orderedmap")
        }
        "rush_event_quest_folder" => {
            include_bytes!("../../assets/cn-activity-masters/rush_event_quest_folder.orderedmap")
        }
        _ => return None,
    })
}
