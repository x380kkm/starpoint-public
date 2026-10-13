// audience: internal
// # personal-service-cn-login-bonus-catalog
// 主表奖励组按日期和回归资格选择, 已开始的有限奖励组继续保留领取位置.

use super::GroupState;
use crate::cn_battle_assets::Reward;
use crate::database::parse_iso_timestamp;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct Catalog {
    pub(super) groups: BTreeMap<String, Vec<LoginBonus>>,
}

#[derive(Deserialize)]
pub(super) struct LoginBonus {
    pub(super) index: i64,
    pub(super) group_type: i64,
    pub(super) name: Option<String>,
    start_time: i64,
    end_time: Option<i64>,
    condition_start_time: Option<i64>,
    condition_end_time: Option<i64>,
    comeback_days: Option<i64>,
    comeback_group: Option<String>,
    include_beginner: bool,
    pub(super) reason_id: i64,
    pub(super) rewards: Vec<Reward>,
}

static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

pub(super) fn definitions() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-login-bonus.json"))
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN login bonus definitions: {error}"))
        })
}

// //// 根据登录时刻和已开始的奖励组确定领取资格 [@x380kkm 2026-09-22] ////
impl Catalog {
    pub(super) fn eligible_groups(
        &self,
        root: &Map<String, Value>,
        stored: &BTreeMap<String, GroupState>,
        last_login: Option<i64>,
        now: i64,
    ) -> Vec<String> {
        let normal_group = self
            .groups
            .iter()
            .filter(|(_, rows)| {
                let row = &rows[0];
                row.group_type == 0 && row.is_open(now)
            })
            .min_by_key(|(_, rows)| rows[0].start_time)
            .map(|(id, _)| id);
        self.groups
            .iter()
            .filter_map(|(id, rows)| {
                let row = &rows[0];
                if !row.is_open(now) || (row.group_type == 0 && Some(id) != normal_group) {
                    return None;
                }
                let eligible = stored
                    .get(id)
                    .is_some_and(|state| state.index > 0)
                    || match row.group_type {
                        0 | 1 => true,
                        2 | 5 | 6 => row.is_comeback_eligible(root, last_login, now),
                        3 => {
                            root.get("user_info")
                                .and_then(|info| info.get("is_comeback"))
                                .and_then(Value::as_bool)
                                == Some(true)
                        }
                        4 => !row
                            .comeback_group
                            .as_ref()
                            .filter(|group| *group != id)
                            .and_then(|group| self.groups.get(group))
                            .is_some_and(|rows| {
                                rows[0].is_comeback_eligible(root, last_login, now)
                            }),
                        _ => false,
                    };
                eligible.then(|| id.clone())
            })
            .collect()
    }
}

impl LoginBonus {
    fn is_open(&self, now: i64) -> bool {
        now >= self.start_time && self.end_time.map_or(true, |end| now <= end)
    }

    fn is_comeback_eligible(
        &self,
        root: &Map<String, Value>,
        last_login: Option<i64>,
        now: i64,
    ) -> bool {
        let user = root.get("user_info");
        if user
            .and_then(|user| user.get("is_newbie"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return self.include_beginner;
        }
        last_login.is_some_and(|last| {
            self.condition_start_time
                .map_or(true, |start| last >= start)
                && self.condition_end_time.map_or(true, |end| last <= end)
                && self
                    .comeback_days
                    .is_some_and(|days| now.saturating_sub(last) >= days * 86_400)
        })
    }

    pub(super) fn group_type_name(&self) -> &'static str {
        match self.group_type {
            0 => "Normal",
            1 => "Limited",
            2 => "Comeback",
            3 => "ComebackAlways",
            4 => "ActiveUser",
            5 => "ComebackCn",
            6 => "ComebackJp",
            _ => unreachable!(),
        }
    }
}
// //// /根据登录时刻和已开始的奖励组确定领取资格 ////

// //// 读取存档的 UTC 登录时间 [@x380kkm 2026-09-22] ////
pub(super) fn stored_login_time(root: &Map<String, Value>) -> Option<i64> {
    root.get("user_info")
        .and_then(|user| user.get("last_login_time"))
        .and_then(Value::as_str)
        .and_then(|time| parse_iso_timestamp(&format!("{}Z", time.replace(' ', "T"))))
        .map(|time| time.div_euclid(1000))
}
