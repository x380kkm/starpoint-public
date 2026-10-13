// audience: internal
// # personal-service-cn-multiplayer-attention
//
// BOT 房主在真人进入大厅并准备后通过现有 Started 帧启动下一局.
// 当前局的结算结束后, 真人重新进入大厅才产生新的准备状态.

use super::transport::{all_human_members_ready, lobby_roster};
use super::{MultiplayerSessionListener, SessionState};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::json;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

impl MultiplayerSessionListener {
    // //// 在真人准备完成后推进 BOT 房主开战 [@x380kkm 2026-09-23] ////
    pub(super) fn poll_attention_lobbies(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        let connected = self
            .clients
            .iter()
            .filter(|client| !client.peer_closed && !client.close_after_write)
            .filter_map(|client| match &client.state {
                SessionState::Lobby {
                    room_number,
                    room_sequence,
                    ..
                } => Some((room_number.clone(), *room_sequence)),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        self.attention_start_deadlines
            .retain(|key, _| connected.contains(key));
        let mut did_work = false;
        let now = Instant::now();
        for key in connected {
            let Some(room) = database.multiplayer_room(&key.0)? else {
                continue;
            };
            if !room.is_attention_room
                || room.room_sequence != key.1
                || room.battle_started
                || room.lobby_started
            {
                continue;
            }
            if !crate::cn_multi::can_start_attention_room(database, &room)? {
                database.disband_multiplayer_room(&room.room_number, room.host_account_id)?;
                self.attention_start_deadlines.remove(&key);
                did_work = true;
                continue;
            }
            if !self.npc_ready_rooms.contains(&key)
                || !all_human_members_ready(database, &room.room_number)?
            {
                self.attention_start_deadlines.remove(&key);
                continue;
            }
            let Some(deadline) = self.attention_start_deadlines.get(&key) else {
                self.broadcast_start_remaining_time(&room.room_number, room.room_sequence, 2)?;
                self.attention_start_deadlines
                    .insert(key, now + Duration::from_secs(2));
                did_work = true;
                continue;
            };
            if *deadline > now {
                continue;
            }
            let expected = database
                .list_multiplayer_members(&room.room_number)?
                .into_iter()
                .filter(|member| member.entered)
                .map(|member| member.viewer_id)
                .collect::<BTreeSet<_>>();
            if database.stage_multiplayer_battle_expected_viewers(
                &room.room_number,
                room.room_sequence,
                room.host_account_id,
                &expected,
            )? && database
                .set_multiplayer_lobby_started(&room.room_number, room.host_account_id)?
            {
                let roster = lobby_roster(database, &room.room_number, true, true)?;
                self.broadcast_lobby(
                    &room.room_number,
                    room.room_sequence,
                    &json!([1, [5, roster]]),
                )?;
                did_work = true;
            }
            self.attention_start_deadlines.remove(&key);
        }
        Ok(did_work)
    }
}
