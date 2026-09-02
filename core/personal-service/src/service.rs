// audience: internal
// # personal-service-lifecycle
//
// 该模块拥有个人服务线程和生命周期命令. 事件循环驱动 HTTP, 本地联机会话, 自动存档和传输
// 绑定调度. 外部调用方串行使用同一句柄.

use crate::cn_multiplayer::MultiplayerSessionListener;
use crate::database::ServiceDatabase;
use crate::http::LoopbackServer;
use crate::local_saves::{SaveAutomationRunner, TransferBindingRunner};
use crate::PersonalServiceError;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

enum ServiceCommand {
    Flush(Sender<Result<(), String>>),
    Stop(Sender<Result<(), String>>),
}

struct ReadyService {
    port: u16,
    multiplayer_session_port: Option<u16>,
    management_token: String,
}

// //// 维护个人服务线程运行状态 [@x380kkm 2026-08-24] ////
struct ServiceRunningState<'a> {
    is_running: &'a AtomicBool,
}

impl<'a> ServiceRunningState<'a> {
    fn enter(is_running: &'a AtomicBool) -> Self {
        is_running.store(true, Ordering::Release);
        Self { is_running }
    }
}

impl Drop for ServiceRunningState<'_> {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Release);
    }
}
// //// /维护个人服务线程运行状态 ////

pub struct PersonalService {
    commands: Sender<ServiceCommand>,
    is_running: Arc<AtomicBool>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
    port: u16,
    multiplayer_session_port: Option<u16>,
    management_token: String,
}

pub struct PersonalServiceOptions {
    root_path: PathBuf,
    cn_asset_root: PathBuf,
    cn_override_root: PathBuf,
    port: u16,
    log_http_access: bool,
    multiplayer_session_enabled: bool,
    multiplayer_session_port: u16,
}

impl PersonalServiceOptions {
    // //// 创建默认关闭 HTTP 访问日志的个人服务配置 [@x380kkm 2026-08-12] ////
    pub fn new(root_path: impl AsRef<Path>, port: u16, cn_asset_root: impl AsRef<Path>) -> Self {
        let root_path = root_path.as_ref().to_path_buf();
        let cn_override_root = root_path.join("cdn").join("override");
        Self {
            root_path,
            cn_asset_root: cn_asset_root.as_ref().to_path_buf(),
            cn_override_root,
            port,
            log_http_access: false,
            multiplayer_session_enabled: port != 0,
            multiplayer_session_port: 17_172,
        }
    }
    // //// /创建默认关闭 HTTP 访问日志的个人服务配置 ////

    // //// 启用不记录正文、查询参数和请求头的 HTTP 访问日志 [@x380kkm 2026-08-12] ////
    pub fn with_http_access_log(mut self) -> Self {
        self.log_http_access = true;
        self
    }
    // //// /启用不记录正文、查询参数和请求头的 HTTP 访问日志 ////

    // //// 启用固定端口本地联机会话监听器 [@x380kkm 2026-08-22] ////
    pub fn with_multiplayer_session_listener(mut self) -> Self {
        self.multiplayer_session_enabled = true;
        self
    }
    // //// /启用固定端口本地联机会话监听器 ////

    // //// 设置本地联机会话监听端口 [@x380kkm 2026-08-22] ////
    pub fn with_multiplayer_session_port(mut self, port: u16) -> Self {
        self.multiplayer_session_enabled = true;
        self.multiplayer_session_port = port;
        self
    }
    // //// /设置本地联机会话监听端口 ////
}

impl PersonalService {
    // //// 启动 loopback 服务和持久化存储 [@x380kkm 2026-07-22] ////
    pub fn start(root_path: impl AsRef<Path>, port: u16) -> Result<Self, PersonalServiceError> {
        let root_path = root_path.as_ref().to_path_buf();
        let cn_asset_root = root_path.join("cdn").join("cn");
        Self::start_with_cdn_root(root_path, port, cn_asset_root)
    }
    // //// /启动 loopback 服务和持久化存储 ////

    // //// 使用显式 CN CDN 根目录启动 loopback 服务 [@x380kkm 2026-08-11] ////
    pub fn start_with_cdn_root(
        root_path: impl AsRef<Path>,
        port: u16,
        cn_asset_root: impl AsRef<Path>,
    ) -> Result<Self, PersonalServiceError> {
        Self::start_with_options(PersonalServiceOptions::new(root_path, port, cn_asset_root))
    }
    // //// /使用显式 CN CDN 根目录启动 loopback 服务 ////

    // //// 使用完整配置启动 loopback 服务 [@x380kkm 2026-08-12] ////
    pub fn start_with_options(
        options: PersonalServiceOptions,
    ) -> Result<Self, PersonalServiceError> {
        let PersonalServiceOptions {
            root_path,
            cn_asset_root,
            cn_override_root,
            port,
            log_http_access,
            multiplayer_session_enabled,
            multiplayer_session_port,
        } = options;
        if cn_asset_root.as_os_str().is_empty() {
            return Err(PersonalServiceError::new("CN asset root must not be empty"));
        }
        let (command_sender, command_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let is_running = Arc::new(AtomicBool::new(false));
        let thread_running = Arc::clone(&is_running);
        let join_handle = thread::Builder::new()
            .name("starpoint-personal-service".to_owned())
            .spawn(move || {
                let result = run_service(
                    root_path,
                    port,
                    cn_asset_root,
                    cn_override_root,
                    log_http_access,
                    multiplayer_session_enabled,
                    multiplayer_session_port,
                    command_receiver,
                    &thread_running,
                    &ready_sender,
                );
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error.to_string()));
                }
            })
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to start service thread: {error}"))
            })?;

        let ready_service = match ready_receiver.recv_timeout(COMMAND_TIMEOUT) {
            Ok(Ok(service)) => service,
            Ok(Err(error)) => {
                let _ = join_handle.join();
                return Err(PersonalServiceError::new(error));
            }
            Err(error) => {
                let _ = join_handle.join();
                return Err(PersonalServiceError::new(format!(
                    "service startup timed out: {error}"
                )));
            }
        };

        Ok(Self {
            commands: command_sender,
            is_running,
            join_handle: Mutex::new(Some(join_handle)),
            port: ready_service.port,
            multiplayer_session_port: ready_service.multiplayer_session_port,
            management_token: ready_service.management_token,
        })
    }
    // //// /使用完整配置启动 loopback 服务 ////

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    pub fn management_token(&self) -> &str {
        &self.management_token
    }

    pub fn multiplayer_session_port(&self) -> Option<u16> {
        self.multiplayer_session_port
    }

    // //// 在应用暂停前提交数据库状态 [@x380kkm 2026-07-22] ////
    pub fn flush(&self) -> Result<(), PersonalServiceError> {
        let (result_sender, result_receiver) = mpsc::channel();
        self.commands
            .send(ServiceCommand::Flush(result_sender))
            .map_err(|error| {
                PersonalServiceError::new(format!("service is not running: {error}"))
            })?;
        result_receiver
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|error| {
                PersonalServiceError::new(format!("database flush timed out: {error}"))
            })?
            .map_err(PersonalServiceError::new)
    }
    // //// /在应用暂停前提交数据库状态 ////

    // //// 停止服务并等待数据库关闭 [@x380kkm 2026-07-22] ////
    pub fn stop(mut self) -> Result<(), PersonalServiceError> {
        self.stop_in_place()
    }

    fn stop_in_place(&mut self) -> Result<(), PersonalServiceError> {
        let mut stop_error = None;
        if self.is_running() {
            let (result_sender, result_receiver) = mpsc::channel();
            if let Err(error) = self.commands.send(ServiceCommand::Stop(result_sender)) {
                stop_error = Some(PersonalServiceError::new(format!(
                    "failed to stop service: {error}"
                )));
            } else {
                match result_receiver.recv_timeout(COMMAND_TIMEOUT) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => stop_error = Some(PersonalServiceError::new(error)),
                    Err(error) => {
                        stop_error = Some(PersonalServiceError::new(format!(
                            "service stop timed out: {error}"
                        )))
                    }
                }
            }
        }

        if let Some(join_handle) = self
            .join_handle
            .lock()
            .map_err(|_| PersonalServiceError::new("service thread lock is poisoned"))?
            .take()
        {
            if join_handle.join().is_err() && stop_error.is_none() {
                stop_error = Some(PersonalServiceError::new("service thread panicked"));
            }
        }

        match stop_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    // //// /停止服务并等待数据库关闭 ////
}

impl Drop for PersonalService {
    fn drop(&mut self) {
        let _ = self.stop_in_place();
    }
}

// //// 运行个人服务事件循环 [@x380kkm 2026-07-22] ////
fn run_service(
    root_path: PathBuf,
    requested_port: u16,
    cn_asset_root: PathBuf,
    cn_override_root: PathBuf,
    log_http_access: bool,
    multiplayer_session_enabled: bool,
    multiplayer_session_port: u16,
    commands: Receiver<ServiceCommand>,
    is_running: &AtomicBool,
    ready: &mpsc::SyncSender<Result<ReadyService, String>>,
) -> Result<(), PersonalServiceError> {
    let mut database = ServiceDatabase::open(&root_path)?;
    fs::create_dir_all(&cn_override_root).map_err(|error| {
        PersonalServiceError::new(format!("failed to create CDN override root: {error}"))
    })?;
    if let Some(initial_time_ms) =
        crate::activity_catalog::initial_activity_calendar_time(&cn_asset_root)
    {
        database.initialize_virtual_time_if_pristine(initial_time_ms)?;
    }
    let mut server = LoopbackServer::bind(
        requested_port,
        cn_asset_root,
        cn_override_root,
        log_http_access,
    )?;
    let mut multiplayer_session = if multiplayer_session_enabled {
        Some(MultiplayerSessionListener::bind(multiplayer_session_port)?)
    } else {
        None
    };
    let active_multiplayer_session_port = match &multiplayer_session {
        Some(multiplayer_session) => Some(multiplayer_session.port()?),
        None => None,
    };
    if let Some(port) = active_multiplayer_session_port {
        database.set_multiplayer_session_port(port);
    }
    let mut save_automation = SaveAutomationRunner::new();
    let mut transfer_bindings = TransferBindingRunner::new();
    let _running_state = ServiceRunningState::enter(is_running);
    ready
        .send(Ok(ReadyService {
            port: server.port(),
            multiplayer_session_port: active_multiplayer_session_port,
            management_token: database.management_token().to_owned(),
        }))
        .map_err(|error| {
            PersonalServiceError::new(format!("failed to report service readiness: {error}"))
        })?;

    loop {
        match commands.try_recv() {
            Ok(ServiceCommand::Flush(result_sender)) => {
                let result = database.checkpoint().map_err(|error| error.to_string());
                let _ = result_sender.send(result);
            }
            Ok(ServiceCommand::Stop(result_sender)) => {
                let result = database.checkpoint().map_err(|error| error.to_string());
                let _ = result_sender.send(result);
                return Ok(());
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                database.checkpoint()?;
                return Ok(());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }

        if let Err(error) = save_automation.poll(&mut database) {
            eprintln!("automatic save task failed: {error}");
        }
        if let Err(error) = transfer_bindings.poll(&mut database) {
            eprintln!("transfer binding task failed: {error}");
        }

        let multiplayer_work = match &mut multiplayer_session {
            Some(multiplayer_session) => multiplayer_session.poll(&mut database)?,
            None => false,
        };
        if !multiplayer_work && !server.try_handle_next_request(&mut database)? {
            thread::sleep(Duration::from_millis(5));
        }
    }
}
// //// /运行个人服务事件循环 ////
