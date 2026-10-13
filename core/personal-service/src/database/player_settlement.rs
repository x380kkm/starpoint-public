// audience: internal
// # personal-service-player-settlement
// 玩家任务, 资产, 邮件和历史共享外层保存点. 内层保存点的提交保留在整次操作的事务内.

use super::{database_error, ServiceDatabase};
use crate::PersonalServiceError;

struct SettlementGuard<'a> {
    database: &'a mut ServiceDatabase,
    active: bool,
}

impl ServiceDatabase {
    // //// 将玩家操作的全部写入作为同一事务提交 [@x380kkm 2026-09-22] ////
    pub(crate) fn with_player_settlement<T>(
        &mut self,
        settle: impl FnOnce(&mut ServiceDatabase) -> Result<Option<T>, PersonalServiceError>,
    ) -> Result<Option<T>, PersonalServiceError> {
        self.connection
            .execute_batch("SAVEPOINT cn_player_settlement")
            .map_err(database_error)?;
        let mut guard = SettlementGuard {
            database: self,
            active: true,
        };
        let result = settle(guard.database);
        match result {
            Ok(Some(value)) => {
                guard
                    .database
                    .connection
                    .execute_batch("RELEASE cn_player_settlement")
                    .map_err(database_error)?;
                guard.active = false;
                Ok(Some(value))
            }
            Ok(None) => {
                guard.rollback()?;
                Ok(None)
            }
            Err(error) => {
                guard.rollback()?;
                Err(error)
            }
        }
    }
    // //// /将玩家操作的全部写入作为同一事务提交 ////
}

impl SettlementGuard<'_> {
    fn rollback(&mut self) -> Result<(), PersonalServiceError> {
        self.database
            .connection
            .execute_batch("ROLLBACK TO cn_player_settlement; RELEASE cn_player_settlement")
            .map_err(database_error)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for SettlementGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.rollback();
        }
    }
}
