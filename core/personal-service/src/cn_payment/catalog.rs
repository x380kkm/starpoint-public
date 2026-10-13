// audience: internal
// # personal-service-cn-payment-catalog
//
// 商品内容来自客户端支付主表, 周期权益来自主表描述和 PremiumLoginBonus 主表.
// 星导石即时金额与日领奖励分别保存, 付费字段表示客户端货币分类.

use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CATALOG_ASSET: &str = include_str!("../../assets/cn-payment-catalog.json");
static CATALOG: OnceLock<Result<PaymentCatalog, String>> = OnceLock::new();

#[derive(Debug, Deserialize)]
pub(crate) struct PaymentCatalog {
    pub(crate) products: BTreeMap<String, PaymentProduct>,
    pub(crate) subscriptions: BTreeMap<String, SubscriptionDefinition>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PaymentProduct {
    pub(crate) store_product_id: String,
    pub(crate) name: String,
    pub(crate) charge_vmoney_num: i64,
    pub(crate) free_vmoney_num: i64,
    pub(crate) price: i64,
    pub(crate) start_time: i64,
    pub(crate) end_time: i64,
    pub(crate) reason_id: Option<i64>,
    pub(crate) rewards: Vec<PaymentReward>,
    pub(crate) entitlements: Vec<PaymentEntitlement>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PaymentReward {
    Item { id: i64, amount: i64 },
    FreeVmoney { amount: i64 },
    Experience { amount: i64 },
    Mana { amount: i64 },
    StarCrumb { amount: i64 },
    Character { id: i64, amount: i64 },
    PassCardPoint { amount: i64 },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "entitlement", rename_all = "snake_case")]
pub(crate) enum PaymentEntitlement {
    GrowthFund { kind: GrowthFundKind },
    PassCard,
    Subscription { id: String },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GrowthFundKind {
    Laite,
    Laite2,
    Laite3,
    MainQuest,
    ExQuest,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubscriptionKind {
    MonthCard,
    MiniMonthCard,
    WeeklyBossBoost,
    WeeklyDailyChallenge,
    PremiumLoginBonus,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionDefinition {
    pub(crate) kind: SubscriptionKind,
    pub(crate) duration_days: i64,
    pub(crate) daily_rewards: Vec<PaymentReward>,
    pub(crate) daily_quest_boost_amount: i64,
    pub(crate) boss_battle_boost_amount: i64,
    pub(crate) daily_challenge_amount: i64,
    pub(crate) enable_auto_3x: bool,
    pub(crate) reason_id: Option<i64>,
    pub(crate) activation_payment_total: Option<i64>,
    pub(crate) catch_up_rewards: bool,
}

// //// 读取完整商品与周期权益目录 [@x380kkm 2026-09-22] ////
pub(crate) fn catalog() -> Result<&'static PaymentCatalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(CATALOG_ASSET)
                .map_err(|error| format!("failed to decode CN payment catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

impl PaymentProduct {
    // //// 生成支付目录的客户端字段 [@x380kkm 2026-09-22] ////
    pub(crate) fn item_list_entry(&self, purchased_times: Option<i64>, period: (i64, i64)) -> Value {
        json!({
            "store_product_id": self.store_product_id,
            "start_time": period.0,
            "end_time": period.1,
            "age_limit": false,
            "monthly_alert": false,
            "purchased_times": purchased_times,
        })
    }
}

impl GrowthFundKind {
    // //// 定位成长基金的玩家资格字段 [@x380kkm 2026-09-22] ////
    pub(crate) fn user_flag(self) -> &'static str {
        match self {
            Self::Laite => "is_bought_fund_laite",
            Self::Laite2 => "is_bought_fund_laite2",
            Self::Laite3 => "is_bought_fund_laite3",
            Self::MainQuest => "is_bought_fund_main_quest",
            Self::ExQuest => "is_bought_fund_ex_quest",
        }
    }
}
