// audience: internal
// # personal-service-cn-party-storage
// 编队类别分别定位普通队伍, 资料队伍与当前活动队伍. 活动队伍沿用接口的数组存档结构.

use super::{get_or_create_party_mut, PartyInventory, PartyLocation};
use crate::cn_profile::{profile_party_groups_mut, PROFILE_PARTY_CATEGORY};
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(crate) trait PartyStorage {
    fn save_party(
        &mut self,
        id: i64,
        party: Map<String, Value>,
    ) -> Result<bool, PersonalServiceError>;
    fn set_group_color(&mut self, id: i64, color: i64) -> bool;
    fn finish_edit(&mut self, _inventory: &PartyInventory) {}
}

struct IndexedParties<'a> {
    groups: &'a mut Map<String, Value>,
}

struct ActivityParties<'a> {
    groups: &'a mut Vec<Value>,
}

struct RaidParties<'a> {
    parties: ActivityParties<'a>,
    edited_party_ids: BTreeSet<i64>,
}

// //// 按客户端类别选择编队存储 [@x380kkm 2026-09-23] ////
pub(crate) fn open(
    root: &mut Map<String, Value>,
    category: i64,
) -> Result<Option<Box<dyn PartyStorage + '_>>, PersonalServiceError> {
    let storage: Box<dyn PartyStorage> = match category {
        1 => {
            let groups = crate::cn_tutorial::require_object(root, "user_party_group_list")?;
            Box::new(IndexedParties { groups })
        }
        PROFILE_PARTY_CATEGORY => Box::new(IndexedParties {
            groups: profile_party_groups_mut(root)?,
        }),
        2 => activity(
            root,
            "carnival",
            1,
            crate::cn_activity::carnival_party_groups,
        )?,
        3 => Box::new(RaidParties {
            parties: ActivityParties {
                groups: activity_groups(root, "raid", 0, crate::cn_activity::raid_party_groups)?,
            },
            edited_party_ids: BTreeSet::new(),
        }),
        4 => activity(root, "rush", 0, crate::cn_activity::rush_party_groups)?,
        _ => return Ok(None),
    };
    Ok(Some(storage))
}

type ActivityPartyLoader =
    fn(&mut Map<String, Value>, &str, i64) -> Result<Value, PersonalServiceError>;

fn activity<'a>(
    root: &'a mut Map<String, Value>,
    family: &str,
    default_event_id: i64,
    load: ActivityPartyLoader,
) -> Result<Box<dyn PartyStorage + 'a>, PersonalServiceError> {
    let groups = activity_groups(root, family, default_event_id, load)?;
    Ok(Box::new(ActivityParties { groups }))
}

fn activity_groups<'a>(
    root: &'a mut Map<String, Value>,
    family: &str,
    default_event_id: i64,
    load: ActivityPartyLoader,
) -> Result<&'a mut Vec<Value>, PersonalServiceError> {
    let event_id = crate::cn_activity::current_event_id(root, family).unwrap_or(default_event_id);
    load(root, family, event_id)?;
    let groups = crate::cn_activity::event_state_mut(root, family, event_id)?
        .get_mut("party_group_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN activity party groups are invalid"))?;
    Ok(groups)
}

// //// 保存索引结构的编队和分组颜色 [@x380kkm 2026-09-23] ////
impl PartyStorage for IndexedParties<'_> {
    fn save_party(
        &mut self,
        id: i64,
        party: Map<String, Value>,
    ) -> Result<bool, PersonalServiceError> {
        get_or_create_party_mut(self.groups, PartyLocation::from_global_id(id))?.extend(party);
        Ok(true)
    }

    fn set_group_color(&mut self, id: i64, color: i64) -> bool {
        let Some(group) = self
            .groups
            .get_mut(&id.to_string())
            .and_then(Value::as_object_mut)
        else {
            return false;
        };
        group.insert("color_id".to_owned(), Value::from(color));
        true
    }
}
// //// /保存索引结构的编队和分组颜色 ////

// //// 保存活动数组中的编队和分组颜色 [@x380kkm 2026-09-23] ////
impl PartyStorage for ActivityParties<'_> {
    fn save_party(
        &mut self,
        id: i64,
        mut party: Map<String, Value>,
    ) -> Result<bool, PersonalServiceError> {
        let stored = self
            .groups
            .iter_mut()
            .filter_map(|group| group.get_mut("party_list").and_then(Value::as_array_mut))
            .flatten()
            .find(|value| value.get("party_id").and_then(Value::as_i64) == Some(id))
            .and_then(Value::as_object_mut);
        let Some(stored) = stored else {
            return Ok(false);
        };
        for (source, target) in [("name", "party_name"), ("edited", "party_edited")] {
            if let Some(value) = party.remove(source) {
                party.insert(target.to_owned(), value);
            }
        }
        stored.extend(party);
        Ok(true)
    }

    fn set_group_color(&mut self, id: i64, color: i64) -> bool {
        let group = self
            .groups
            .iter_mut()
            .find(|group| group.get("party_group_id").and_then(Value::as_i64) == Some(id))
            .and_then(Value::as_object_mut);
        let Some(group) = group else { return false };
        group.insert("party_group_color_id".to_owned(), Value::from(color));
        true
    }
}
// //// /保存活动数组中的编队和分组颜色 ////

// //// 汇总共斗编辑并保留其他队伍的角色引用 [@x380kkm 2026-09-23] ////
impl PartyStorage for RaidParties<'_> {
    fn save_party(
        &mut self,
        id: i64,
        party: Map<String, Value>,
    ) -> Result<bool, PersonalServiceError> {
        let saved = self.parties.save_party(id, party)?;
        if saved {
            self.edited_party_ids.insert(id);
        }
        Ok(saved)
    }

    fn set_group_color(&mut self, id: i64, color: i64) -> bool {
        self.parties.set_group_color(id, color)
    }

    fn finish_edit(&mut self, inventory: &PartyInventory) {
        crate::cn_activity::normalize_raid_party_groups(
            self.parties.groups,
            inventory,
            &self.edited_party_ids,
        );
    }
}
// //// /汇总共斗编辑并保留其他队伍的角色引用 ////
