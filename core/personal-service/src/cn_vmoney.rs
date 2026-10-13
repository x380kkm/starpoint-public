// audience: internal
// # personal-service-cn-vmoney
// 普通星导石消费先使用免费余额, 剩余费用由付费余额补足.

use crate::cn_tutorial::{set_user_info_value, user_info_value};
use crate::PersonalServiceError;
use serde_json::{Map, Value};

pub(crate) struct VirtualMoneyBalance {
    pub(crate) free: i64,
    pub(crate) paid: i64,
}

// //// 按免费优先顺序扣除足额星导石 [@x380kkm 2026-09-26] ////
pub(crate) fn spend(
    root: &mut Map<String, Value>,
    amount: i64,
) -> Result<Option<VirtualMoneyBalance>, PersonalServiceError> {
    let free = user_info_value(root, "free_vmoney")?;
    let paid = user_info_value(root, "vmoney")?;
    if free < 0 || paid < 0 || amount < 0 {
        return Err(PersonalServiceError::new(
            "CN virtual money balance or cost is negative",
        ));
    }
    let free_spent = free.min(amount);
    let paid_spent = amount - free_spent;
    if paid < paid_spent {
        return Ok(None);
    }
    let remaining = VirtualMoneyBalance {
        free: free - free_spent,
        paid: paid - paid_spent,
    };
    set_user_info_value(root, "free_vmoney", remaining.free)?;
    set_user_info_value(root, "vmoney", remaining.paid)?;
    Ok(Some(remaining))
}
// //// /按免费优先顺序扣除足额星导石 ////
