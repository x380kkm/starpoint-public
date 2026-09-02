// audience: internal
// # personal-service-cn-multiplayer-session
//
// 该模块在 127.0.0.1:17172 轮询 NUL 分隔 UTF-8 JSON 大厅和战斗会话.
// 房间许可, 成员状态, connection id 和 COM 冻结载荷由 SQLite 提供.

mod battle;
mod lifecycle;
mod lobby_player;
mod meeting;
mod transport;

use self::lifecycle::PendingLobbySequence;
use self::transport::{flush_client, read_client_frames};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const BATTLE_RECONNECT_GRACE: Duration = Duration::from_secs(2);

enum SessionState {
    Handshake,
    Lobby {
        room_number: String,
        room_sequence: i64,
        viewer_id: i64,
        account_id: i64,
        connection_id: String,
        is_host: bool,
        legacy_protocol: bool,
    },
    Battle {
        room_number: String,
        room_sequence: i64,
        viewer_id: i64,
        connection_id: String,
        scene_ready: bool,
        finalized: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingRoomNoticeDelivery {
    room_number: String,
    room_sequence: i64,
    deadline_ms: i64,
    flush_after_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingBattleStartDelivery {
    room_number: String,
    room_sequence: i64,
    viewer_id: i64,
    flush_after_bytes: u64,
}

struct SessionClient {
    stream: TcpStream,
    buffer: Vec<u8>,
    pending_write: Vec<u8>,
    state: SessionState,
    peer_closed: bool,
    close_after_write: bool,
    queued_write_bytes: u64,
    flushed_write_bytes: u64,
    pending_room_notice_deliveries: Vec<PendingRoomNoticeDelivery>,
    pending_battle_start_deliveries: Vec<PendingBattleStartDelivery>,
}

struct PendingBattleDisconnect {
    room_number: String,
    room_sequence: i64,
    viewer_id: i64,
    connection_id: String,
    deadline: Instant,
}

pub(crate) struct MultiplayerSessionListener {
    listener: TcpListener,
    clients: Vec<SessionClient>,
    pending_battle_disconnects: Vec<PendingBattleDisconnect>,
    pending_lobby_sequences: Vec<PendingLobbySequence>,
    npc_ready_rooms: std::collections::BTreeSet<(String, i64)>,
    auto_starting_rooms: std::collections::BTreeSet<(String, i64)>,
    next_room_event_poll: Instant,
}

impl MultiplayerSessionListener {
    // //// 绑定本地联机会话端口 [@x380kkm 2026-08-22] ////
    pub(crate) fn bind(port: u16) -> Result<Self, PersonalServiceError> {
        let listener =
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to bind multiplayer session listener on 127.0.0.1:{port}: {error}"
                ))
            })?;
        listener.set_nonblocking(true).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to configure multiplayer session listener: {error}"
            ))
        })?;
        Ok(Self {
            listener,
            clients: Vec::new(),
            pending_battle_disconnects: Vec::new(),
            pending_lobby_sequences: Vec::new(),
            npc_ready_rooms: std::collections::BTreeSet::new(),
            auto_starting_rooms: std::collections::BTreeSet::new(),
            next_room_event_poll: Instant::now(),
        })
    }
    // //// /绑定本地联机会话端口 ////

    pub(crate) fn port(&self) -> Result<u16, PersonalServiceError> {
        self.listener
            .local_addr()
            .map(|address| address.port())
            .map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to read multiplayer session listener port: {error}"
                ))
            })
    }

    // //// 轮询联机连接和业务帧 [@x380kkm 2026-08-22] ////
    pub(crate) fn poll(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        let mut did_work = self.accept_clients()?;
        let mut frames = Vec::new();
        for (client_index, client) in self.clients.iter_mut().enumerate() {
            did_work |= read_client_frames(client_index, client, &mut frames)?;
        }
        for (client_index, frame) in frames {
            if client_index >= self.clients.len() || self.clients[client_index].peer_closed {
                continue;
            }
            did_work = true;
            self.handle_frame(client_index, frame, database)?;
        }
        did_work |= self.poll_timed_work(database)?;
        for client in &mut self.clients {
            did_work |= flush_client(client)?;
        }
        did_work |= self.acknowledge_flushed_deliveries(database)?;
        self.remove_closed_clients(database)?;
        Ok(did_work)
    }
    // //// /轮询联机连接和业务帧 ////

    fn accept_clients(&mut self) -> Result<bool, PersonalServiceError> {
        let mut accepted = false;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).map_err(|error| {
                        PersonalServiceError::new(format!(
                            "failed to configure multiplayer client socket: {error}"
                        ))
                    })?;
                    self.clients.push(SessionClient {
                        stream,
                        buffer: Vec::new(),
                        pending_write: Vec::new(),
                        state: SessionState::Handshake,
                        peer_closed: false,
                        close_after_write: false,
                        queued_write_bytes: 0,
                        flushed_write_bytes: 0,
                        pending_room_notice_deliveries: Vec::new(),
                        pending_battle_start_deliveries: Vec::new(),
                    });
                    accepted = true;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(accepted),
                Err(error) => {
                    return Err(PersonalServiceError::new(format!(
                        "failed to accept multiplayer session: {error}"
                    )));
                }
            }
        }
    }

    fn handle_frame(
        &mut self,
        client_index: usize,
        frame: Value,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        match &self.clients[client_index].state {
            SessionState::Handshake => self.handle_handshake(client_index, frame, database),
            SessionState::Lobby { .. } => self.handle_lobby(client_index, frame, database),
            SessionState::Battle { .. } => self.handle_battle(client_index, frame, database),
        }
    }

    fn close_client(&mut self, client_index: usize) -> Result<(), PersonalServiceError> {
        self.clients[client_index].close_after_write = true;
        Ok(())
    }

    fn remove_closed_clients(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        self.flush_expired_battle_disconnects(database)?;
        let now = Instant::now();
        let leaving_battles = self
            .clients
            .iter()
            .filter(|client| client.peer_closed && client.pending_write.is_empty())
            .filter_map(|client| match &client.state {
                SessionState::Battle {
                    room_number,
                    room_sequence,
                    viewer_id,
                    connection_id,
                    finalized,
                    ..
                } if !finalized => Some((
                    room_number.clone(),
                    *room_sequence,
                    *viewer_id,
                    connection_id.clone(),
                )),
                _ => None,
            })
            .filter(|(room_number, _, _, connection_id)| {
                !self.clients.iter().any(|client| {
                    !client.peer_closed
                        && matches!(
                            &client.state,
                            SessionState::Battle {
                                room_number: active_room,
                                connection_id: active_connection,
                                ..
                            } if active_room == room_number && active_connection == connection_id
                        )
                })
            })
            .collect::<Vec<_>>();
        let leaving_lobbies = self
            .clients
            .iter()
            .filter(|client| client.peer_closed && client.pending_write.is_empty())
            .filter_map(|client| match &client.state {
                SessionState::Lobby {
                    room_number,
                    room_sequence,
                    viewer_id,
                    connection_id,
                    ..
                } => Some((
                    room_number.clone(),
                    *room_sequence,
                    *viewer_id,
                    connection_id.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (room_number, room_sequence, viewer_id, connection_id) in leaving_battles {
            let already_pending = self.pending_battle_disconnects.iter().any(|disconnect| {
                disconnect.room_number == room_number
                    && disconnect.room_sequence == room_sequence
                    && disconnect.connection_id == connection_id
            });
            if !already_pending {
                self.pending_battle_disconnects
                    .push(PendingBattleDisconnect {
                        room_number,
                        room_sequence,
                        viewer_id,
                        connection_id,
                        deadline: now + BATTLE_RECONNECT_GRACE,
                    });
            }
        }
        for (room_number, room_sequence, viewer_id, connection_id) in leaving_lobbies {
            let left = database.leave_multiplayer_lobby(&room_number, viewer_id, &connection_id)?;
            let room = database.multiplayer_room(&room_number)?;
            if left
                && room
                    .as_ref()
                    .is_some_and(|room| room.room_sequence == room_sequence && !room.battle_started)
            {
                let include_ai = room.as_ref().is_some_and(|room| room.is_npc_mode);
                let roster = transport::lobby_roster(database, &room_number, include_ai, false)?;
                self.broadcast_lobby(&room_number, room_sequence, &json!([1, [1, roster]]))?;
            }
        }
        self.clients
            .retain(|client| !(client.peer_closed && client.pending_write.is_empty()));
        Ok(())
    }

    fn flush_expired_battle_disconnects(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        let now = Instant::now();
        let pending = std::mem::take(&mut self.pending_battle_disconnects);
        for disconnect in pending {
            let reconnected = self.clients.iter().any(|client| {
                !client.peer_closed
                    && matches!(
                        &client.state,
                        SessionState::Battle {
                            room_number,
                            connection_id,
                            ..
                        } if room_number == &disconnect.room_number
                            && connection_id == &disconnect.connection_id
                    )
            });
            if reconnected {
                continue;
            }
            if disconnect.deadline > now {
                self.pending_battle_disconnects.push(disconnect);
                continue;
            }
            let suspended = database.suspend_multiplayer_battle_expected_viewer(
                &disconnect.room_number,
                disconnect.room_sequence,
                disconnect.viewer_id,
            )?;
            self.broadcast_battle(
                &disconnect.room_number,
                disconnect.room_sequence,
                &json!([1, [0, disconnect.connection_id]]),
            )?;
            if suspended {
                self.start_battle_if_ready(
                    database,
                    &disconnect.room_number,
                    disconnect.room_sequence,
                )?;
            }
        }
        Ok(())
    }
}
