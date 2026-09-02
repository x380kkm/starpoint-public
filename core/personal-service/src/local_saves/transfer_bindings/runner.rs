// audience: internal
// # transfer-binding-runner
//
// 该模块在个人服务前台运行时执行到期的 interval 绑定.
// 远端传输线程不访问 SQLite, 同时最多运行一个绑定传输.

use super::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const BINDING_POLL_INTERVAL: Duration = Duration::from_secs(1);

struct ActiveTransfer {
    binding_id: String,
    receiver: Receiver<CompletedTransferBindingSync>,
    handle: Option<JoinHandle<()>>,
}

pub(crate) struct TransferBindingRunner {
    next_poll: Instant,
    active: Option<ActiveTransfer>,
}

// //// 轮询并执行一个到期绑定 [@x380kkm 2026-08-03] ////
impl TransferBindingRunner {
    pub(crate) fn new() -> Self {
        Self {
            next_poll: Instant::now(),
            active: None,
        }
    }

    pub(crate) fn poll(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        self.finish_transfer(database)?;
        let now = Instant::now();
        if now < self.next_poll || self.active.is_some() {
            return Ok(());
        }
        self.next_poll = now + BINDING_POLL_INTERVAL;
        let Some(binding_id) = database.list_due_transfer_binding_ids()?.into_iter().next() else {
            return Ok(());
        };
        let prepared = match prepare_transfer_binding_sync(
            database,
            &binding_id,
            TransferSyncTrigger::Interval,
        ) {
            Ok(prepared) => prepared,
            Err(TransferOperationError::Storage(error)) => return Err(error),
            Err(_) => return Ok(()),
        };
        let (sender, receiver) = mpsc::channel();
        let handle = match thread::Builder::new()
            .name("starpoint-transfer-binding".to_owned())
            .spawn(move || {
                let _ = sender.send(execute_prepared_transfer_binding_sync(prepared));
            }) {
            Ok(handle) => handle,
            Err(_) => {
                record_worker_failure(database, &binding_id)?;
                return Ok(());
            }
        };
        self.active = Some(ActiveTransfer {
            binding_id,
            receiver,
            handle: Some(handle),
        });
        Ok(())
    }
    fn finish_transfer(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        let completed = match active.receiver.try_recv() {
            Ok(completed) => completed,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                record_worker_failure(database, &active.binding_id)?;
                self.join_finished_transfer();
                return Ok(());
            }
        };
        match commit_completed_transfer_binding_sync(database, completed) {
            Ok(_) => {}
            Err(TransferOperationError::Storage(error)) => return Err(error),
            Err(_) => {}
        }
        self.join_finished_transfer();
        Ok(())
    }

    fn join_finished_transfer(&mut self) {
        if let Some(mut active) = self.active.take() {
            if let Some(handle) = active.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for TransferBindingRunner {
    fn drop(&mut self) {
        let Some(mut active) = self.active.take() else {
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
// //// /轮询并执行一个到期绑定 ////

// //// 记录传输线程启动或通道失败 [@x380kkm 2026-08-03] ////
fn record_worker_failure(
    database: &mut ServiceDatabase,
    binding_id: &str,
) -> Result<(), PersonalServiceError> {
    let Some(binding) = database.get_transfer_binding(binding_id)? else {
        return Ok(());
    };
    let Some(source) = create_local_transfer_save(database, binding.source_slot_id)? else {
        return Ok(());
    };
    database
        .record_transfer_binding_failure(
            binding_id,
            binding.revision,
            &source.etag,
            None,
            "transfer_worker_failed",
            TRANSFER_RETRY_SECONDS,
        )
        .map_err(|error| match error {
            TransferBindingStoreError::Storage(error) => error,
            _ => PersonalServiceError::new("failed to record transfer worker failure"),
        })
}
// //// /记录传输线程启动或通道失败 ////
