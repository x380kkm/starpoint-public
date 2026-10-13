// audience: internal
// # personal-service-multiplayer-ai-source
// 保存快照保留外键和账号归属, 内置模板通过模板编号冻结到房间.

use super::multiplayer_database_error;
use crate::PersonalServiceError;
use rusqlite::Connection;

const CREATE_MATES: &str = "CREATE TABLE IF NOT EXISTS multiplayer_ai_mates (
    room_number TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position IN (1, 2)),
    owner_account_id INTEGER NOT NULL,
    snapshot_id TEXT,
    client_mate_json TEXT NOT NULL,
    lobby_player_json TEXT NOT NULL,
    template_index INTEGER,
    CHECK ((snapshot_id IS NOT NULL AND template_index IS NULL)
        OR (snapshot_id IS NULL AND template_index IS NOT NULL AND template_index IN (0, 1))),
    PRIMARY KEY (room_number, position),
    UNIQUE (room_number, snapshot_id),
    UNIQUE (room_number, template_index),
    FOREIGN KEY (room_number) REFERENCES multiplayer_rooms (room_number) ON DELETE CASCADE,
    FOREIGN KEY (owner_account_id) REFERENCES accounts (id) ON DELETE CASCADE,
    FOREIGN KEY (snapshot_id) REFERENCES ai_team_snapshots (id) ON DELETE RESTRICT
);";

// //// 保留房间 AI 内容并区分保存快照与内置模板 [@x380kkm 2026-09-27] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(CREATE_MATES)
        .map_err(multiplayer_database_error)?;
    let requires_snapshot: bool = connection.query_row(
        "SELECT \"notnull\" FROM pragma_table_info('multiplayer_ai_mates') WHERE name='snapshot_id'",
        [], |row| row.get(0),
    ).map_err(multiplayer_database_error)?;
    if requires_snapshot {
        let transaction = connection
            .unchecked_transaction()
            .map_err(multiplayer_database_error)?;
        transaction
            .execute_batch(
                "ALTER TABLE multiplayer_ai_mates RENAME TO multiplayer_ai_mates_with_snapshots;",
            )
            .map_err(multiplayer_database_error)?;
        transaction
            .execute_batch(CREATE_MATES)
            .map_err(multiplayer_database_error)?;
        transaction.execute_batch(
            "INSERT INTO multiplayer_ai_mates (room_number, position, owner_account_id, snapshot_id, client_mate_json, lobby_player_json)
             SELECT room_number, position, owner_account_id, snapshot_id, client_mate_json, lobby_player_json
             FROM multiplayer_ai_mates_with_snapshots;
             DROP TABLE multiplayer_ai_mates_with_snapshots;",
        ).map_err(multiplayer_database_error)?;
        transaction.commit().map_err(multiplayer_database_error)?;
    }
    Ok(())
}
// //// /保留房间 AI 内容并区分保存快照与内置模板 ////

#[cfg(test)]
mod tests {
    use super::*;

    // //// 迁移保存快照房间并保存原始载荷和外键 [@x380kkm 2026-09-27] ////
    #[test]
    fn retains_frozen_snapshot_and_foreign_keys_during_source_migration() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE multiplayer_rooms(room_number TEXT PRIMARY KEY);
             CREATE TABLE accounts(id INTEGER PRIMARY KEY);
             CREATE TABLE ai_team_snapshots(id TEXT PRIMARY KEY);
             INSERT INTO multiplayer_rooms VALUES('123456');
             INSERT INTO accounts VALUES(1);
             INSERT INTO ai_team_snapshots VALUES('saved-team');
             CREATE TABLE multiplayer_ai_mates (
                 room_number TEXT NOT NULL, position INTEGER NOT NULL CHECK(position IN(1,2)),
                 owner_account_id INTEGER NOT NULL, snapshot_id TEXT NOT NULL,
                 client_mate_json TEXT NOT NULL, lobby_player_json TEXT NOT NULL,
                 PRIMARY KEY(room_number,position), UNIQUE(room_number,snapshot_id),
                 FOREIGN KEY(room_number) REFERENCES multiplayer_rooms(room_number) ON DELETE CASCADE,
                 FOREIGN KEY(owner_account_id) REFERENCES accounts(id) ON DELETE CASCADE,
                 FOREIGN KEY(snapshot_id) REFERENCES ai_team_snapshots(id) ON DELETE RESTRICT
             );
             INSERT INTO multiplayer_ai_mates VALUES('123456',1,1,'saved-team','client-payload','lobby-payload');",
        ).unwrap();
        migrate(&connection).unwrap();
        migrate(&connection).unwrap();
        let retained: (String, String, String, Option<i64>) = connection.query_row(
            "SELECT snapshot_id,client_mate_json,lobby_player_json,template_index FROM multiplayer_ai_mates",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).unwrap();
        assert_eq!(
            retained,
            (
                "saved-team".into(),
                "client-payload".into(),
                "lobby-payload".into(),
                None
            )
        );
        assert!(connection
            .execute("DELETE FROM ai_team_snapshots WHERE id='saved-team'", [])
            .is_err());
        connection.execute("INSERT INTO multiplayer_ai_mates VALUES('123456',2,1,NULL,'default-client','default-lobby',1)", []).unwrap();
        assert!(connection
            .execute(
                "UPDATE multiplayer_ai_mates SET template_index=NULL WHERE position=2",
                []
            )
            .is_err());
        assert!(connection
            .execute(
                "UPDATE multiplayer_ai_mates SET template_index=0 WHERE position=1",
                []
            )
            .is_err());
        connection
            .execute(
                "DELETE FROM multiplayer_rooms WHERE room_number='123456'",
                [],
            )
            .unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM multiplayer_ai_mates", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }
}
