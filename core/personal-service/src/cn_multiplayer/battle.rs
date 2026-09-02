// audience: internal
// # personal-service-cn-multiplayer-battle
//
// 该模块处理联机战斗准备, 结束, 单播, 广播和测量帧.

use super::transport::queue_frame;
use super::{MultiplayerSessionListener, PendingBattleStartDelivery, SessionState};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Value};
use std::collections::BTreeSet;

impl MultiplayerSessionListener {
    pub(super) fn handle_battle(
        &mut self,
        client_index: usize,
        frame: Value,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        let (room_number, room_sequence, viewer_id, connection_id) =
            match &self.clients[client_index].state {
                SessionState::Battle {
                    room_number,
                    room_sequence,
                    viewer_id,
                    connection_id,
                    ..
                } => (
                    room_number.clone(),
                    *room_sequence,
                    *viewer_id,
                    connection_id.clone(),
                ),
                _ => return Ok(()),
            };
        let Some(data) = frame.as_array() else {
            return self.close_client(client_index);
        };
        match data.first().and_then(Value::as_i64) {
            Some(0) => {
                if data.len() != 2 {
                    return self.close_client(client_index);
                }
                let Some(notify) = data[1].as_array() else {
                    return self.close_client(client_index);
                };
                match notify.first().and_then(Value::as_i64) {
                    Some(0) if notify.len() == 1 => {
                        if let SessionState::Battle { scene_ready, .. } =
                            &mut self.clients[client_index].state
                        {
                            *scene_ready = true;
                        }
                        database.set_multiplayer_member_scene_ready(
                            &room_number,
                            viewer_id,
                            true,
                        )?;
                        self.start_battle_if_ready(database, &room_number, room_sequence)?;
                    }
                    Some(1) if notify.len() == 1 => {}
                    Some(2) if notify.len() == 1 => {
                        if let SessionState::Battle { finalized, .. } =
                            &mut self.clients[client_index].state
                        {
                            *finalized = true;
                        }
                        queue_frame(&mut self.clients[client_index], &json!([1, [2]]))?;
                    }
                    Some(3) if notify.len() == 3 => {
                        let (frame_count, client_time) = flat_battle_measurement(notify);
                        self.send_battle_measurement_ack(
                            client_index,
                            database,
                            frame_count,
                            client_time,
                        )?;
                    }
                    Some(4)
                        if notify.len() == 2 && notify[1].as_f64().is_some_and(f64::is_finite) => {}
                    Some(5) if notify.len() == 1 => {}
                    Some(_) => {}
                    None => {}
                }
            }
            Some(1) => {
                if data.len() != 2 || !valid_broadcast_messages(&data[1]) {
                    return self.close_client(client_index);
                }
                let messages = data[1].clone();
                let outgoing = json!([2, connection_id, messages]);
                self.broadcast_battle(&room_number, room_sequence, &outgoing)?;
            }
            Some(2) => {
                if data.len() != 3 || !valid_send_message(&data[2]) {
                    return self.close_client(client_index);
                }
                let valid_targets = data[1].as_array().is_some_and(|targets| {
                    !targets.is_empty()
                        && targets.len() <= 32
                        && targets.iter().all(|target| {
                            target.as_str().is_some_and(|connection_id| {
                                !connection_id.is_empty() && connection_id.len() <= 128
                            })
                        })
                });
                if !valid_targets {
                    return self.close_client(client_index);
                }
                let target_connection_ids = data[1]
                    .as_array()
                    .expect("validated battle targets are an array")
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>();
                let message = data[2].clone();
                self.send_battle_to_connections(
                    &room_number,
                    room_sequence,
                    &target_connection_ids,
                    &json!([3, connection_id, message]),
                )?;
            }
            Some(_) => {}
            None => return self.close_client(client_index),
        }
        Ok(())
    }

    fn send_battle_measurement_ack(
        &mut self,
        client_index: usize,
        database: &ServiceDatabase,
        frame_count: i64,
        client_time: f64,
    ) -> Result<(), PersonalServiceError> {
        queue_frame(
            &mut self.clients[client_index],
            &json!([
                1,
                [
                    3,
                    frame_count,
                    client_time,
                    database.current_server_time_millis()?
                ]
            ]),
        )
    }

    fn send_battle_to_connections(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        target_connection_ids: &BTreeSet<String>,
        frame: &Value,
    ) -> Result<(), PersonalServiceError> {
        for client in &mut self.clients {
            if matches!(
                &client.state,
                SessionState::Battle {
                    room_number: client_room,
                    room_sequence: client_sequence,
                    connection_id,
                    ..
                } if client_room == room_number
                    && *client_sequence == room_sequence
                    && target_connection_ids.contains(connection_id)
            ) && !client.peer_closed
            {
                queue_frame(client, frame)?;
            }
        }
        Ok(())
    }

    pub(super) fn broadcast_battle(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        frame: &Value,
    ) -> Result<(), PersonalServiceError> {
        for client in &mut self.clients {
            if matches!(
                &client.state,
                SessionState::Battle {
                    room_number: client_room,
                    room_sequence: client_sequence,
                    ..
                } if client_room == room_number && *client_sequence == room_sequence
            ) && !client.peer_closed
            {
                queue_frame(client, frame)?;
            }
        }
        Ok(())
    }

    pub(super) fn start_battle_if_ready(
        &mut self,
        database: &mut ServiceDatabase,
        room_number: &str,
        room_sequence: i64,
    ) -> Result<(), PersonalServiceError> {
        let expected_viewers =
            database.multiplayer_battle_expected_viewers(room_number, room_sequence)?;
        if expected_viewers.is_empty() {
            return Ok(());
        }
        let mut ready_viewers = std::collections::BTreeSet::new();
        for client in &self.clients {
            if let SessionState::Battle {
                room_number: client_room,
                room_sequence: client_sequence,
                viewer_id,
                scene_ready,
                ..
            } = &client.state
            {
                if client_room == room_number
                    && *client_sequence == room_sequence
                    && !client.peer_closed
                {
                    if *scene_ready {
                        ready_viewers.insert(*viewer_id);
                    }
                }
            }
        }
        if expected_viewers.is_subset(&ready_viewers) {
            let undelivered_viewers =
                database.multiplayer_battle_undelivered_viewers(room_number, room_sequence)?;
            for client in &mut self.clients {
                let viewer_id = match &client.state {
                    SessionState::Battle {
                        room_number: client_room,
                        room_sequence: client_sequence,
                        viewer_id,
                        ..
                    } if client_room == room_number
                        && *client_sequence == room_sequence
                        && !client.peer_closed =>
                    {
                        Some(*viewer_id)
                    }
                    _ => None,
                };
                let Some(viewer_id) = viewer_id else {
                    continue;
                };
                let delivery_pending =
                    client
                        .pending_battle_start_deliveries
                        .iter()
                        .any(|delivery| {
                            delivery.room_number == room_number
                                && delivery.room_sequence == room_sequence
                                && delivery.viewer_id == viewer_id
                        });
                if !undelivered_viewers.contains(&viewer_id) || delivery_pending {
                    continue;
                }
                queue_frame(client, &json!([1, [1]]))?;
                client
                    .pending_battle_start_deliveries
                    .push(PendingBattleStartDelivery {
                        room_number: room_number.to_owned(),
                        room_sequence,
                        viewer_id,
                        flush_after_bytes: client.queued_write_bytes,
                    });
            }
        }
        Ok(())
    }
}

fn flat_battle_measurement(notify: &[Value]) -> (i64, f64) {
    let frame_count = notify.get(1).and_then(Value::as_i64).unwrap_or_default();
    let client_time = notify
        .get(2)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or_default();
    (frame_count, client_time)
}

fn valid_broadcast_messages(value: &Value) -> bool {
    value.as_array().is_some_and(|messages| {
        messages.iter().all(|message| {
            message.as_array().is_some_and(|message| {
                message.len() == 6
                    && message[0].as_i64() == Some(0)
                    && message[1..5]
                        .iter()
                        .all(|value| value.as_i64().is_some_and(|value| value >= 0))
                    && (message[5].is_null() || message[5].is_string())
            })
        })
    })
}

fn valid_send_message(value: &Value) -> bool {
    value.as_array().is_some_and(|message| {
        message.len() == 2
            && message[0].as_i64() == Some(0)
            && message[1]
                .as_array()
                .is_some_and(|command| !command.is_empty() && command[0].as_i64().is_some())
    })
}
