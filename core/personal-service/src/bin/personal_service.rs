// audience: external
// # personal-service
//
// 该进程托管可部署的个人服务. 服务只监听 IPv4 loopback, root 和端口由命令行或环境变量提供.
// 进程从标准输入读取 stop 或 quit, 也响应 Ctrl-C 和 SIGTERM 后提交状态并退出.

use starpoint_personal_service::{PersonalService, PersonalServiceOptions};
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const DEFAULT_ROOT: &str = "data/personal-service";
const DEFAULT_PORT: u16 = 17_171;
const DEFAULT_MULTIPLAYER_SESSION_PORT: u16 = 17_172;
const CDN_ROOT_ENVIRONMENT_VARIABLE: &str = "STARPOINT_PERSONAL_SERVICE_CDN_ROOT";

struct Arguments {
    root: PathBuf,
    cn_asset_root: PathBuf,
    port: u16,
    multiplayer_session_port: u16,
    show_management_token: bool,
    log_http_access: bool,
}

// //// 解析个人服务进程参数 [@x380kkm 2026-07-24] ////
fn parse_arguments() -> Result<Arguments, String> {
    let mut root = env::var_os("STARPOINT_PERSONAL_SERVICE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ROOT));
    let mut cn_asset_root = env::var_os(CDN_ROOT_ENVIRONMENT_VARIABLE).map(PathBuf::from);
    let mut port = env::var("PERSONAL_SERVICE_PORT")
        .ok()
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| "PERSONAL_SERVICE_PORT 不是有效端口.".to_owned())?
        .unwrap_or(DEFAULT_PORT);
    let mut multiplayer_session_port = env::var("PERSONAL_SERVICE_MULTIPLAYER_SESSION_PORT")
        .ok()
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| "PERSONAL_SERVICE_MULTIPLAYER_SESSION_PORT 不是有效端口.".to_owned())?
        .unwrap_or(DEFAULT_MULTIPLAYER_SESSION_PORT);
    let mut show_management_token = false;
    let mut log_http_access = false;
    let mut arguments = env::args_os().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--root") => {
                root = PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--root 缺少路径.".to_owned())?,
                );
            }
            Some("--cdn-root") => {
                cn_asset_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--cdn-root 缺少路径.".to_owned())?,
                ));
            }
            Some("--port") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--port 缺少端口.".to_owned())?;
                port = value
                    .to_str()
                    .ok_or_else(|| "--port 不是有效文本.".to_owned())?
                    .parse::<u16>()
                    .map_err(|_| "--port 不是有效端口.".to_owned())?;
            }
            Some("--session-port") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--session-port 缺少端口.".to_owned())?;
                multiplayer_session_port = value
                    .to_str()
                    .ok_or_else(|| "--session-port 不是有效文本.".to_owned())?
                    .parse::<u16>()
                    .map_err(|_| "--session-port 不是有效端口.".to_owned())?;
            }
            Some("--show-management-token") => show_management_token = true,
            Some("--log-http-access") => log_http_access = true,
            Some("--help" | "-h") => return Err(usage()),
            Some(value) => return Err(format!("未知参数: {value}\n\n{}", usage())),
            None => return Err("参数不是有效 UTF-8.".to_owned()),
        }
    }

    if root.as_os_str().is_empty() {
        return Err("个人服务 root 不能为空.".to_owned());
    }
    let cn_asset_root = cn_asset_root.unwrap_or_else(|| root.join("cdn").join("cn"));
    if cn_asset_root.as_os_str().is_empty() {
        return Err("CN CDN 根目录不能为空.".to_owned());
    }
    Ok(Arguments {
        root,
        cn_asset_root,
        port,
        multiplayer_session_port,
        show_management_token,
        log_http_access,
    })
}
// //// /解析个人服务进程参数 ////

// //// 返回命令行用法文本 [@x380kkm 2026-07-24] ////
fn usage() -> String {
    "用法: personal-service [--root PATH] [--cdn-root PATH] [--port PORT] [--session-port PORT] [--show-management-token] [--log-http-access]"
        .to_owned()
}
// //// /返回命令行用法文本 ////

// //// 运行个人服务直到收到标准输入停止命令 [@x380kkm 2026-07-24] ////
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    let mut options =
        PersonalServiceOptions::new(&arguments.root, arguments.port, &arguments.cn_asset_root)
            .with_multiplayer_session_port(arguments.multiplayer_session_port);
    if arguments.log_http_access {
        options = options.with_http_access_log();
    }
    let service = PersonalService::start_with_options(options)?;
    println!("personal-service ready");
    println!("root={}", arguments.root.display());
    println!("cdn_root={}", arguments.cn_asset_root.display());
    println!("health=http://127.0.0.1:{}/health", service.port());
    println!("management=http://127.0.0.1:{}/manage", service.port());
    if arguments.show_management_token {
        println!("management_token={}", service.management_token());
    }
    io::stdout().flush()?;

    let (stop_sender, stop_receiver) = mpsc::channel();
    let signal_sender = stop_sender.clone();
    ctrlc::set_handler(move || {
        let _ = signal_sender.send(());
    })
    .map_err(|error| format!("无法安装进程停止处理器: {error}"))?;
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) if matches!(line.trim(), "stop" | "quit") => {
                    let _ = stop_sender.send(());
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    while service.is_running() {
        if stop_receiver
            .recv_timeout(Duration::from_millis(250))
            .is_ok()
        {
            break;
        }
    }
    service.stop()?;
    Ok(())
}
// //// /运行个人服务直到收到标准输入停止命令 ////

// //// 启动正式个人服务进程 [@x380kkm 2026-07-24] ////
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = match parse_arguments() {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("{message}");
            if message == usage() {
                return Ok(());
            }
            return Err(message.into());
        }
    };
    run(arguments)
}
// //// /启动正式个人服务进程 ////
