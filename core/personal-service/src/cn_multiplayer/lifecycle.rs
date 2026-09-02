// audience: internal
// # personal-service-cn-multiplayer-lifecycle
//
// 该模块按单调时钟推进大厅 NPC 帧, 并把持久化房间事件投递到对应会话.
// RemainingTime 和 BattleStart 只在对应 socket 完整写出帧后确认.

use super::transport::{all_human_members_ready, lobby_roster, queue_frame};
use super::{MultiplayerSessionListener, PendingRoomNoticeDelivery, SessionState};
use crate::database::{MultiplayerRoomEvent, MultiplayerRoomEventKind, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::json;
use std::time::{Duration, Instant};

const ROOM_EVENT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const NPC_JOIN_DELAY: Duration = Duration::from_millis(2_000);
const NPC_READY_DELAY: Duration = Duration::from_millis(500);

enum PendingLobbyPhase {
    Join,
    Ready,
}

pub(super) struct PendingLobbySequence {
    room_number: String,
    room_sequence: i64,
    deadline: Instant,
    phase: PendingLobbyPhase,
}

impl MultiplayerSessionListener {
    // //// 安排 NPC 加入和就绪帧 [@x380kkm 2026-08-23] ////
    pub(super) fn schedule_npc_lobby_sequence(&mut self, room_number: String, room_sequence: i64) {
        self.schedule_npc_lobby_sequence_at(room_number, room_sequence, Instant::now());
    }

    fn schedule_npc_lobby_sequence_at(
        &mut self,
        room_number: String,
        room_sequence: i64,
        now: Instant,
    ) {
        self.npc_ready_rooms
            .remove(&(room_number.clone(), room_sequence));
        self.auto_starting_rooms
            .remove(&(room_number.clone(), room_sequence));
        if self.pending_lobby_sequences.iter().any(|sequence| {
            sequence.room_number == room_number && sequence.room_sequence == room_sequence
        }) {
            return;
        }
        self.pending_lobby_sequences
            .retain(|sequence| sequence.room_number != room_number);
        self.pending_lobby_sequences.push(PendingLobbySequence {
            room_number,
            room_sequence,
            deadline: now + NPC_JOIN_DELAY,
            phase: PendingLobbyPhase::Join,
        });
    }
    // //// /安排 NPC 加入和就绪帧 ////

    // //// 推进会话定时状态 [@x380kkm 2026-08-23] ////
    pub(super) fn poll_timed_work(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        let mut did_work = self.poll_pending_lobby_sequences(database)?;
        did_work |= self.poll_room_events(database)?;
        Ok(did_work)
    }
    // //// /推进会话定时状态 ////

    // //// 确认完整写出的持久会话帧 [@x380kkm 2026-08-23] ////
    pub(super) fn acknowledge_flushed_deliveries(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        let mut delivered_room_notices = Vec::new();
        let mut delivered_battle_starts = Vec::new();
        for client in &mut self.clients {
            let flushed_write_bytes = client.flushed_write_bytes;
            let pending = std::mem::take(&mut client.pending_room_notice_deliveries);
            for delivery in pending {
                if delivery.flush_after_bytes <= flushed_write_bytes {
                    delivered_room_notices.push(delivery);
                } else {
                    client.pending_room_notice_deliveries.push(delivery);
                }
            }
            let pending = std::mem::take(&mut client.pending_battle_start_deliveries);
            for delivery in pending {
                if delivery.flush_after_bytes <= flushed_write_bytes {
                    delivered_battle_starts.push(delivery);
                } else {
                    client.pending_battle_start_deliveries.push(delivery);
                }
            }
        }
        let did_work = !delivered_room_notices.is_empty() || !delivered_battle_starts.is_empty();
        for delivery in delivered_room_notices {
            database.mark_multiplayer_remaining_notified(
                &delivery.room_number,
                delivery.room_sequence,
                delivery.deadline_ms,
            )?;
        }
        for delivery in delivered_battle_starts {
            database.mark_multiplayer_battle_start_delivered(
                &delivery.room_number,
                delivery.room_sequence,
                delivery.viewer_id,
            )?;
        }
        Ok(did_work)
    }
    // //// /确认完整写出的持久会话帧 ////

    fn poll_pending_lobby_sequences(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        let now = Instant::now();
        let sequences = std::mem::take(&mut self.pending_lobby_sequences);
        let mut did_work = false;
        for mut sequence in sequences {
            if sequence.deadline > now {
                self.pending_lobby_sequences.push(sequence);
                continue;
            }
            let room = database.multiplayer_room(&sequence.room_number)?;
            let room_is_active = room.as_ref().is_some_and(|room| {
                room.room_sequence == sequence.room_sequence
                    && !room.battle_started
                    && room.raising_state != 4
                    && room.is_npc_mode
            });
            if !room_is_active
                || !self.has_active_host_lobby(&sequence.room_number, sequence.room_sequence)
            {
                continue;
            }
            match sequence.phase {
                PendingLobbyPhase::Join => {
                    let roster = lobby_roster(database, &sequence.room_number, true, false)?;
                    self.broadcast_lobby(
                        &sequence.room_number,
                        sequence.room_sequence,
                        &json!([1, [1, roster]]),
                    )?;
                    sequence.phase = PendingLobbyPhase::Ready;
                    sequence.deadline += NPC_READY_DELAY;
                    self.pending_lobby_sequences.push(sequence);
                    did_work = true;
                }
                PendingLobbyPhase::Ready => {
                    for mate in database.list_multiplayer_ai_mates(&sequence.room_number)? {
                        self.broadcast_lobby(
                            &sequence.room_number,
                            sequence.room_sequence,
                            &json!([
                                1,
                                [
                                    2,
                                    format!("{}-npc-{}", sequence.room_number, mate.position),
                                    [1]
                                ]
                            ]),
                        )?;
                    }
                    self.npc_ready_rooms
                        .insert((sequence.room_number.clone(), sequence.room_sequence));
                    self.evaluate_lobby_readiness(
                        database,
                        &sequence.room_number,
                        sequence.room_sequence,
                    )?;
                    did_work = true;
                }
            }
        }
        Ok(did_work)
    }

    pub(super) fn evaluate_lobby_readiness(
        &mut self,
        database: &mut ServiceDatabase,
        room_number: &str,
        room_sequence: i64,
    ) -> Result<(), PersonalServiceError> {
        let room_key = (room_number.to_owned(), room_sequence);
        if self.pending_lobby_sequences.iter().any(|sequence| {
            sequence.room_number == room_number && sequence.room_sequence == room_sequence
        }) {
            return Ok(());
        }
        let Some(room) = database.multiplayer_room(room_number)? else {
            return Ok(());
        };
        if room.room_sequence != room_sequence || room.lobby_started || room.battle_started {
            return Ok(());
        }
        let npc_ready = self.npc_ready_rooms.contains(&room_key);
        let roster = lobby_roster(
            database,
            room_number,
            room.is_npc_mode && npc_ready,
            npc_ready,
        )?;
        if roster.len() < 2 {
            return Ok(());
        }
        let members = database.list_multiplayer_members(room_number)?;
        let non_host_ready = members
            .iter()
            .filter(|member| member.entered && member.account_id != room.host_account_id)
            .all(|member| member.ready);
        if let Some(host) = members
            .iter()
            .find(|member| member.entered && member.account_id == room.host_account_id)
        {
            if host.ready != non_host_ready {
                database.set_multiplayer_member_ready(
                    room_number,
                    host.viewer_id,
                    non_host_ready,
                )?;
                if let Some(connection_id) = &host.connection_id {
                    self.broadcast_lobby(
                        room_number,
                        room_sequence,
                        &json!([1, [2, connection_id, [i64::from(non_host_ready)]]]),
                    )?;
                }
            }
        }
        if roster.len() >= 3
            && all_human_members_ready(database, room_number)?
            && self.auto_starting_rooms.insert(room_key)
        {
            self.broadcast_start_remaining_time(room_number, room_sequence, 2)?;
        }
        Ok(())
    }

    fn broadcast_start_remaining_time(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        seconds: i64,
    ) -> Result<(), PersonalServiceError> {
        for client in &mut self.clients {
            let legacy_protocol = match &client.state {
                SessionState::Lobby {
                    room_number: client_room,
                    room_sequence: client_sequence,
                    legacy_protocol,
                    ..
                } if client_room == room_number && *client_sequence == room_sequence => {
                    *legacy_protocol
                }
                _ => continue,
            };
            if !client.peer_closed {
                let tag = if legacy_protocol { 10 } else { 9 };
                queue_frame(client, &json!([1, [tag, seconds]]))?;
            }
        }
        Ok(())
    }

    fn poll_room_events(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        let now = Instant::now();
        if now < self.next_room_event_poll {
            return Ok(false);
        }
        self.next_room_event_poll = now + ROOM_EVENT_POLL_INTERVAL;
        let wall_time_ms = database.current_wall_time_millis()?;
        let events: Vec<MultiplayerRoomEvent> =
            database.poll_multiplayer_room_events(wall_time_ms)?;
        let did_work = !events.is_empty();
        for event in events {
            match event.kind {
                MultiplayerRoomEventKind::Remaining {
                    seconds,
                    deadline_ms,
                } => {
                    self.queue_remaining_time(
                        &event.room_number,
                        event.room_sequence,
                        seconds,
                        deadline_ms,
                    )?;
                }
                MultiplayerRoomEventKind::Dismissed => {
                    self.broadcast_lobby(
                        &event.room_number,
                        event.room_sequence,
                        &json!([1, [6, "multibattle_room_dismissed"]]),
                    )?;
                    self.pending_lobby_sequences.retain(|sequence| {
                        sequence.room_number != event.room_number
                            || sequence.room_sequence != event.room_sequence
                    });
                    self.npc_ready_rooms
                        .remove(&(event.room_number.clone(), event.room_sequence));
                    self.auto_starting_rooms
                        .remove(&(event.room_number.clone(), event.room_sequence));
                    for client in &mut self.clients {
                        let belongs_to_room = matches!(
                            &client.state,
                            SessionState::Lobby {
                                room_number,
                                room_sequence,
                                ..
                            } | SessionState::Battle {
                                room_number,
                                room_sequence,
                                ..
                            } if room_number == &event.room_number
                                && *room_sequence == event.room_sequence
                        );
                        if belongs_to_room {
                            client.close_after_write = true;
                        }
                    }
                }
            }
        }
        Ok(did_work)
    }

    fn queue_remaining_time(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        seconds: i64,
        deadline_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        let frame = json!([1, [7, seconds]]);
        let mut queued = false;
        for client in &mut self.clients {
            let belongs_to_room = matches!(
                &client.state,
                SessionState::Lobby {
                    room_number: client_room,
                    room_sequence: client_sequence,
                    ..
                } if client_room == room_number && *client_sequence == room_sequence
            ) && !client.peer_closed;
            if !belongs_to_room
                || client
                    .pending_room_notice_deliveries
                    .iter()
                    .any(|delivery| {
                        delivery.room_number == room_number
                            && delivery.room_sequence == room_sequence
                            && delivery.deadline_ms == deadline_ms
                    })
            {
                continue;
            }
            queue_frame(client, &frame)?;
            client
                .pending_room_notice_deliveries
                .push(PendingRoomNoticeDelivery {
                    room_number: room_number.to_owned(),
                    room_sequence,
                    deadline_ms,
                    flush_after_bytes: client.queued_write_bytes,
                });
            queued = true;
        }
        Ok(queued)
    }

    fn has_active_host_lobby(&self, room_number: &str, room_sequence: i64) -> bool {
        self.clients.iter().any(|client| {
            !client.peer_closed
                && matches!(
                    &client.state,
                    SessionState::Lobby {
                        room_number: client_room,
                        room_sequence: client_sequence,
                        is_host: true,
                        ..
                    } if client_room == room_number && *client_sequence == room_sequence
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::flush_client;
    use super::*;
    use std::collections::BTreeSet;
    use std::net::{TcpListener, TcpStream};
    use tempfile::TempDir;

    // //// 验证重复 EnterComs 保留最早期限 [@x380kkm 2026-08-23] ////
    #[test]
    fn keeps_earliest_deadline_for_duplicate_npc_sequence() {
        let mut listener = MultiplayerSessionListener::bind(0).expect("session listener binds");
        let first_request = Instant::now();
        listener.schedule_npc_lobby_sequence_at("100001".to_owned(), 1, first_request);
        let first_deadline = listener.pending_lobby_sequences[0].deadline;
        listener.schedule_npc_lobby_sequence_at(
            "100001".to_owned(),
            1,
            first_request + Duration::from_secs(1),
        );
        assert_eq!(listener.pending_lobby_sequences.len(), 1);
        assert_eq!(listener.pending_lobby_sequences[0].deadline, first_deadline);
    }
    // //// /验证重复 EnterComs 保留最早期限 ////

    // //// 验证持久会话帧完整写出后才确认 [@x380kkm 2026-08-23] ////
    #[test]
    fn acknowledges_persistent_frames_only_after_complete_flush() {
        let root = TempDir::new().expect("temporary service directory is created");
        let mut database = ServiceDatabase::open(root.path()).expect("service database opens");
        let signup = match database.get_or_create_account_and_rotate_viewer_session(1, "{}") {
            Ok(signup) => signup,
            Err(_) => panic!("test account is created"),
        };
        let expiry_anchor_ms = 1_000_000;
        let room = database
            .create_multiplayer_room(
                signup.account_id,
                signup.viewer_id,
                1,
                1,
                1,
                1_001_002,
                1,
                expiry_anchor_ms,
            )
            .expect("test multiplayer room is created");
        database
            .enter_multiplayer_lobby(&room.room_number, signup.viewer_id, "{}")
            .expect("test host enters the room");
        assert!(database
            .stage_multiplayer_battle_expected_viewers(
                &room.room_number,
                room.room_sequence,
                signup.account_id,
                &BTreeSet::from([signup.viewer_id]),
            )
            .expect("battle viewers are staged"));

        let socket_listener = TcpListener::bind(("127.0.0.1", 0)).expect("test socket binds");
        let receiver = TcpStream::connect(socket_listener.local_addr().unwrap())
            .expect("test receiver connects");
        let (sender, _) = socket_listener.accept().expect("test sender is accepted");
        sender
            .set_nonblocking(true)
            .expect("test sender is nonblocking");
        let mut listener = MultiplayerSessionListener::bind(0).expect("session listener binds");
        listener.clients.push(super::super::SessionClient {
            stream: sender,
            buffer: Vec::new(),
            pending_write: Vec::new(),
            state: SessionState::Lobby {
                room_number: room.room_number.clone(),
                room_sequence: room.room_sequence,
                viewer_id: signup.viewer_id,
                account_id: signup.account_id,
                connection_id: "test-connection".to_owned(),
                is_host: true,
                legacy_protocol: false,
            },
            peer_closed: false,
            close_after_write: false,
            queued_write_bytes: 0,
            flushed_write_bytes: 0,
            pending_room_notice_deliveries: Vec::new(),
            pending_battle_start_deliveries: Vec::new(),
        });

        let deadline_ms = expiry_anchor_ms + 15 * 60 * 1_000;
        let events = database
            .poll_multiplayer_room_events(deadline_ms - 30 * 1_000)
            .expect("remaining notice is polled");
        let event = events.first().expect("remaining notice exists");
        let (seconds, notice_deadline_ms) = match &event.kind {
            MultiplayerRoomEventKind::Remaining {
                seconds,
                deadline_ms,
            } => (*seconds, *deadline_ms),
            MultiplayerRoomEventKind::Dismissed => panic!("room is not expired"),
        };
        assert!(listener
            .queue_remaining_time(
                &room.room_number,
                room.room_sequence,
                seconds,
                notice_deadline_ms,
            )
            .expect("remaining notice is queued"));
        assert_eq!(listener.clients[0].flushed_write_bytes, 0);
        assert_eq!(
            database
                .poll_multiplayer_room_events(deadline_ms - 1)
                .expect("unflushed notice is polled again")
                .len(),
            1
        );

        assert!(flush_client(&mut listener.clients[0]).expect("remaining notice is flushed"));
        assert_eq!(
            listener.clients[0].flushed_write_bytes,
            listener.clients[0].queued_write_bytes
        );
        assert_eq!(
            database
                .poll_multiplayer_room_events(deadline_ms - 1)
                .expect("flushed but unacknowledged notice is polled again")
                .len(),
            1
        );
        assert!(listener
            .acknowledge_flushed_deliveries(&mut database)
            .expect("flushed notice is acknowledged"));
        assert!(database
            .poll_multiplayer_room_events(deadline_ms - 1)
            .expect("acknowledged notice is suppressed")
            .is_empty());
        listener.clients[0].state = SessionState::Battle {
            room_number: room.room_number.clone(),
            room_sequence: room.room_sequence,
            viewer_id: signup.viewer_id,
            connection_id: "test-connection".to_owned(),
            scene_ready: true,
            finalized: false,
        };
        listener
            .start_battle_if_ready(&mut database, &room.room_number, room.room_sequence)
            .expect("battle start is queued");
        let queued_write_bytes = listener.clients[0].queued_write_bytes;
        listener
            .start_battle_if_ready(&mut database, &room.room_number, room.room_sequence)
            .expect("pending battle start is not queued twice");
        assert_eq!(listener.clients[0].queued_write_bytes, queued_write_bytes);
        assert_eq!(listener.clients[0].pending_battle_start_deliveries.len(), 1);
        assert_eq!(
            database
                .multiplayer_battle_undelivered_viewers(&room.room_number, room.room_sequence)
                .expect("unflushed battle start remains pending"),
            BTreeSet::from([signup.viewer_id])
        );
        assert!(flush_client(&mut listener.clients[0]).expect("battle start is flushed"));
        assert!(listener
            .acknowledge_flushed_deliveries(&mut database)
            .expect("flushed battle start is acknowledged"));
        assert!(database
            .multiplayer_battle_undelivered_viewers(&room.room_number, room.room_sequence)
            .expect("acknowledged battle start remains delivered")
            .is_empty());
        drop(receiver);
    }
    // //// /验证持久会话帧完整写出后才确认 ////
}
