// audience: internal
// # personal-service-multiplayer-participation
//
// 战斗任务使用开战时的已接受救援和队友新手快照.
// 参与来源与战斗收据共同提交, 完成请求只读取保存结果.

use crate::database::multiplayer::{multiplayer_database_error, MultiplayerBattleIdentity};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

// //// 保存房间内可信的战斗参与来源 [@x380kkm 2026-09-23] ////
pub(in crate::database::multiplayer) fn save(
    connection: &Connection,
    identity: MultiplayerBattleIdentity<'_>,
) -> Result<(), PersonalServiceError> {
    connection.execute(
        "INSERT OR IGNORE INTO multiplayer_battle_participation
         (account_id, play_id, room_number, is_attention, has_newbie)
         SELECT ?1, ?2, ?3,
             EXISTS(SELECT 1 FROM multiplayer_attention_deliveries AS delivered
                    JOIN multiplayer_room_members AS member
                      ON member.room_number=delivered.room_number AND member.viewer_id=delivered.viewer_id
                    WHERE member.account_id=?1 AND member.room_number=?3 AND delivered.accepted=1),
             EXISTS(SELECT 1 FROM multiplayer_room_members AS mate
                    JOIN player_snapshots AS snapshots ON snapshots.account_id=mate.account_id
                    WHERE mate.room_number=?3 AND mate.account_id<>?1 AND mate.entered=1
                      AND json_extract(snapshots.data_json, '$.user_info.is_newbie')=1)
             OR EXISTS(SELECT 1 FROM multiplayer_ai_mates AS mate
                       WHERE mate.room_number=?3 AND json_extract(mate.lobby_player_json, '$.isNewbie')=1)",
        params![identity.account_id, identity.play_id, identity.room_number],
    ).map(|_| ()).map_err(multiplayer_database_error)
}

impl ServiceDatabase {
    // //// 读取已开战身份对应的救援与新手同行资格 [@x380kkm 2026-09-23] ////
    pub(crate) fn multiplayer_battle_participation(
        &self,
        account_id: i64,
        play_id: &str,
    ) -> Result<(bool, bool), PersonalServiceError> {
        self.connection.query_row(
            "SELECT is_attention, has_newbie FROM multiplayer_battle_participation WHERE account_id=?1 AND play_id=?2",
            params![account_id, play_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map(|value| value.unwrap_or_default()).map_err(multiplayer_database_error)
    }
}
