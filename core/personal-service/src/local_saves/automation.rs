// audience: internal | external
// # local-save-automation
//
// 该模块配置持久化自动快照并在前台服务运行时执行已经到期的任务.
// 远端上传线程不访问 SQLite, 个人服务停止时不等待正在进行的网络请求.

use super::*;
use crate::database::{
    LocalSaveAutomation, LocalSaveAutomationInput, LocalSaveAutomationStoreError,
    DEFAULT_AUTOMATION_INTERVAL_SECONDS, MAX_AUTOMATION_INTERVAL_SECONDS,
    MIN_AUTOMATION_INTERVAL_SECONDS,
};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const AUTOMATION_POLL_INTERVAL: Duration = Duration::from_secs(1);
const AUTOMATIC_SNAPSHOT_LABEL: &str = "Automatic snapshot";
const AUTOMATIC_SNAPSHOT_RETENTION_COUNT: i64 = 48;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AutomationRequest {
    enabled: bool,
    interval_seconds: i64,
    target_id: Option<i64>,
    object_id: Option<String>,
}

#[derive(Serialize)]
struct AutomationResponse {
    slot_id: i64,
    enabled: bool,
    interval_seconds: i64,
    target_id: Option<i64>,
    object_id: Option<String>,
    next_run_at: Option<String>,
    last_snapshot_at: Option<String>,
    last_upload_at: Option<String>,
    last_error: Option<String>,
    pending_upload: bool,
}

struct ActiveUpload {
    identity: sync::SaveUploadIdentity,
    automation_revision: i64,
    receiver: Receiver<sync::CompletedSaveUpload>,
    handle: Option<JoinHandle<()>>,
}

pub(crate) struct SaveAutomationRunner {
    next_poll: Instant,
    active_upload: Option<ActiveUpload>,
}

impl SaveAutomationRunner {
    pub(crate) fn new() -> Self {
        Self {
            next_poll: Instant::now(),
            active_upload: None,
        }
    }

    // //// 执行到期快照并轮询远端上传 [@x380kkm 2026-07-23] ////
    pub(crate) fn poll(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        self.finish_upload(database)?;
        let now = Instant::now();
        if now < self.next_poll {
            return Ok(());
        }
        self.next_poll = now + AUTOMATION_POLL_INTERVAL;

        for automation in database.list_due_local_save_automations()? {
            let mut upload_pending = automation.pending_upload;
            let mut upload_revision = automation.revision;
            if automation.snapshot_due {
                upload_pending = self.create_snapshot(database, &automation)?;
                upload_revision += 1;
            }
            if self.active_upload.is_none() && upload_pending {
                self.start_upload(database, &automation, upload_revision)?;
            }
        }
        Ok(())
    }
    // //// /执行到期快照并轮询远端上传 ////

    fn create_snapshot(
        &self,
        database: &mut ServiceDatabase,
        automation: &LocalSaveAutomation,
    ) -> Result<bool, PersonalServiceError> {
        match database.create_automatic_local_save_snapshot(
            automation.slot_id,
            AUTOMATIC_SNAPSHOT_LABEL,
            AUTOMATIC_SNAPSHOT_RETENTION_COUNT,
        ) {
            Ok(_) => {
                database.record_automatic_snapshot_success(automation.slot_id)?;
                Ok(automation.target_id.is_some() && automation.object_id.is_some())
            }
            Err(error) => {
                let error_code = match error {
                    LocalSaveStoreError::NotFound => "local_save_not_found",
                    LocalSaveStoreError::Busy => "local_save_busy",
                    LocalSaveStoreError::InvalidState => "local_save_invalid_state",
                    LocalSaveStoreError::Storage(_) => "local_save_storage_failed",
                };
                database.record_automatic_snapshot_failure(automation.slot_id, error_code)?;
                Ok(false)
            }
        }
    }

    fn start_upload(
        &mut self,
        database: &mut ServiceDatabase,
        automation: &LocalSaveAutomation,
        automation_revision: i64,
    ) -> Result<(), PersonalServiceError> {
        let (Some(target_id), Some(object_id)) =
            (automation.target_id, automation.object_id.as_deref())
        else {
            return Ok(());
        };
        let prepared =
            match sync::prepare_save_upload(database, automation.slot_id, target_id, object_id) {
                Ok(prepared) => prepared,
                Err(error) => {
                    database.record_automatic_upload_failure(
                        automation.slot_id,
                        target_id,
                        object_id,
                        automation_revision,
                        error.code(),
                    )?;
                    return Ok(());
                }
            };
        let identity = prepared.identity.clone();
        let (sender, receiver) = mpsc::channel();
        let handle = match thread::Builder::new()
            .name("starpoint-save-upload".to_owned())
            .spawn(move || {
                let _ = sender.send(sync::execute_prepared_save_upload(prepared));
            }) {
            Ok(handle) => handle,
            Err(_) => {
                database.schedule_automatic_upload_retry(
                    identity.slot_id,
                    identity.target_id,
                    &identity.object_id,
                    automation_revision,
                    "save_sync_upload_worker_failed",
                )?;
                return Ok(());
            }
        };
        self.active_upload = Some(ActiveUpload {
            identity,
            automation_revision,
            receiver,
            handle: Some(handle),
        });
        Ok(())
    }

    fn finish_upload(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        let Some(active) = self.active_upload.as_mut() else {
            return Ok(());
        };
        let result = match active.receiver.try_recv() {
            Ok(completed) => sync::commit_completed_save_upload(database, completed),
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                database.schedule_automatic_upload_retry(
                    active.identity.slot_id,
                    active.identity.target_id,
                    &active.identity.object_id,
                    active.automation_revision,
                    "save_sync_upload_worker_failed",
                )?;
                self.join_finished_upload();
                return Ok(());
            }
        };
        match result {
            Ok(_) => database.record_automatic_upload_success(
                active.identity.slot_id,
                active.identity.target_id,
                &active.identity.object_id,
                active.automation_revision,
            )?,
            Err(error) => {
                if error.is_retryable() {
                    database.schedule_automatic_upload_retry(
                        active.identity.slot_id,
                        active.identity.target_id,
                        &active.identity.object_id,
                        active.automation_revision,
                        error.code(),
                    )?;
                } else {
                    database.record_automatic_upload_failure(
                        active.identity.slot_id,
                        active.identity.target_id,
                        &active.identity.object_id,
                        active.automation_revision,
                        error.code(),
                    )?;
                }
            }
        }
        self.join_finished_upload();
        Ok(())
    }

    fn join_finished_upload(&mut self) {
        if let Some(mut active) = self.active_upload.take() {
            if let Some(handle) = active.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for SaveAutomationRunner {
    fn drop(&mut self) {
        let Some(mut active) = self.active_upload.take() else {
            return;
        };
        let Some(handle) = active.handle.take() else {
            return;
        };
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

// //// 分派自动快照配置请求 [@x380kkm 2026-07-23] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let prefix = format!("{LOCAL_SAVES_PATH}/");
    let segments = request
        .path()
        .strip_prefix(&prefix)
        .unwrap_or_default()
        .split('/')
        .collect::<Vec<_>>();
    match (request.method(), segments.as_slice()) {
        ("GET", [slot_id, "automation"]) => Some(get_automation(database, slot_id)),
        ("PUT", [slot_id, "automation"]) => Some(set_automation(request, database, slot_id)),
        (_, [_, "automation"]) => Some(Ok(method_not_allowed())),
        _ => None,
    }
}
// //// /分派自动快照配置请求 ////

fn get_automation(
    database: &ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    match database.get_local_save_automation(slot_id) {
        Ok(Some(automation)) => serialize_json("200 OK", response(automation)),
        Ok(None) => serialize_json("200 OK", default_response(slot_id)),
        Err(error) => map_store_error(error),
    }
}

fn set_automation(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<AutomationRequest>(request))
    else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_local_save_automation",
        ));
    };
    let has_valid_interval = (MIN_AUTOMATION_INTERVAL_SECONDS..=MAX_AUTOMATION_INTERVAL_SECONDS)
        .contains(&body.interval_seconds);
    let has_valid_upload = match (body.target_id, body.object_id.as_deref()) {
        (None, None) => true,
        (Some(target_id), Some(object_id)) => target_id > 0 && sync::is_valid_object_id(object_id),
        _ => false,
    };
    if !has_valid_interval || !has_valid_upload {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_local_save_automation",
        ));
    }
    let input = LocalSaveAutomationInput {
        enabled: body.enabled,
        interval_seconds: body.interval_seconds,
        target_id: body.target_id,
        object_id: body.object_id,
    };
    match database.set_local_save_automation(slot_id, &input) {
        Ok(automation) => serialize_json("200 OK", response(automation)),
        Err(error) => map_store_error(error),
    }
}

fn response(automation: LocalSaveAutomation) -> AutomationResponse {
    AutomationResponse {
        slot_id: automation.slot_id,
        enabled: automation.enabled,
        interval_seconds: automation.interval_seconds,
        target_id: automation.target_id,
        object_id: automation.object_id,
        next_run_at: Some(automation.next_run_at),
        last_snapshot_at: automation.last_snapshot_at,
        last_upload_at: automation.last_upload_at,
        last_error: automation.last_error,
        pending_upload: automation.pending_upload,
    }
}

fn default_response(slot_id: i64) -> AutomationResponse {
    AutomationResponse {
        slot_id,
        enabled: false,
        interval_seconds: DEFAULT_AUTOMATION_INTERVAL_SECONDS,
        target_id: None,
        object_id: None,
        next_run_at: None,
        last_snapshot_at: None,
        last_upload_at: None,
        last_error: None,
        pending_upload: false,
    }
}

fn map_store_error(
    error: LocalSaveAutomationStoreError,
) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        LocalSaveAutomationStoreError::LocalSaveNotFound => {
            Ok(json_error("404 Not Found", "local_save_not_found"))
        }
        LocalSaveAutomationStoreError::TargetNotFound => {
            Ok(json_error("404 Not Found", "save_sync_target_not_found"))
        }
        LocalSaveAutomationStoreError::Storage(error) => Err(error),
    }
}
