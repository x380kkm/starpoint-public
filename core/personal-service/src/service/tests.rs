// audience: internal
// # service-lifecycle-tests
//
// 测试覆盖初始化交付, 取消后释放监听端口, 以及联机活动期间的 HTTP 响应.

use super::*;
use std::io::{ErrorKind, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::time::Instant;

// //// 在持续联机连接期间响应 HTTP 请求 [@x380kkm 2026-10-02] ////
#[test]
fn serves_http_while_multiplayer_has_work_on_every_poll() {
    let root = tempfile::TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let mut server = LoopbackServer::bind(
        0,
        root.path().join("cdn"),
        root.path().join("overrides"),
        false,
    )
    .unwrap();
    let listener = MultiplayerSessionListener::bind(0).unwrap();
    let session_port = listener.port().unwrap();
    let mut multiplayer = Some(listener);
    let mut http = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    http.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    http.set_nonblocking(true).unwrap();
    let mut clients = Vec::new();
    let mut response = [0_u8; 12];
    for _ in 0..100 {
        clients.push(TcpStream::connect(("127.0.0.1", session_port)).unwrap());
        assert!(poll_network_requests(&mut database, &mut server, &mut multiplayer).unwrap());
        match http.peek(&mut response) {
            Ok(12) => {
                assert_eq!(&response, b"HTTP/1.1 200");
                return;
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => panic!("HTTP connection failed during multiplayer work: {error}"),
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("HTTP request remains queued while multiplayer accepts new connections");
}
// //// /在持续联机连接期间响应 HTTP 请求 ////

// //// 创建由通道控制启动时机的服务线程 [@x380kkm 2026-09-10] ////
fn spawn_pending_service(
    root: PathBuf,
    port: u16,
    ready_capacity: usize,
    start_gate: Receiver<()>,
) -> (PendingService, Receiver<()>) {
    let (commands, command_receiver) = mpsc::channel();
    let (ready_sender, ready) = mpsc::sync_channel(ready_capacity);
    let (done_sender, done) = mpsc::channel();
    let is_running = Arc::new(AtomicBool::new(false));
    let thread_running = Arc::clone(&is_running);
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = Arc::clone(&cancelled);
    let join_handle = thread::spawn(move || {
        if start_gate.recv().is_ok() {
            let result = run_service(
                root.clone(),
                port,
                root.join("cdn"),
                root.join("override"),
                false,
                false,
                0,
                command_receiver,
                &thread_running,
                &ready_sender,
                &thread_cancelled,
            );
            if let Err(error) = result {
                let _ = ready_sender.try_send(Err(error.to_string()));
            }
        }
        let _ = done_sender.send(());
    });
    (
        PendingService {
            commands,
            ready,
            is_running,
            cancelled,
            join_handle,
        },
        done,
    )
}
// //// /创建由通道控制启动时机的服务线程 ////

// //// 启动超时在初始化完成前返回并取消线程 [@x380kkm 2026-09-10] ////
#[test]
fn startup_timeout_returns_before_delayed_initialization_finishes() {
    let root = tempfile::tempdir().expect("temporary state directory");
    let (release_start, start_gate) = mpsc::channel();
    let (pending, done) = spawn_pending_service(root.path().to_owned(), 0, 1, start_gate);
    let cancelled = Arc::clone(&pending.cancelled);
    let (outcome_sender, outcome_receiver) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let outcome = pending.wait(Duration::ZERO);
        let _ = outcome_sender.send(outcome.map(|service| service.stop()));
    });

    let outcome = outcome_receiver.recv_timeout(Duration::from_secs(5));
    let cancellation_was_requested = cancelled.load(Ordering::Acquire);
    cancelled.store(true, Ordering::Release);
    release_start.send(()).expect("release initialization");
    done.recv_timeout(Duration::from_secs(5))
        .expect("cancelled startup exits");
    waiter.join().expect("startup waiter exits");

    assert!(cancellation_was_requested);
    let error = outcome
        .expect("timeout returns while worker is waiting")
        .expect_err("startup is not delivered");
    assert!(error
        .to_string()
        .contains("service startup did not complete"));
    assert!(!root.path().join("personal-service.sqlite3").exists());
}
// //// /启动超时在初始化完成前返回并取消线程 ////

// //// 取消就绪交付后释放实际监听端口 [@x380kkm 2026-09-10] ////
#[test]
fn cancelled_readiness_releases_the_loopback_listener() {
    for ready_capacity in [0, 1] {
        let root = tempfile::tempdir().expect("temporary state directory");
        let reservation = TcpListener::bind(("127.0.0.1", 0)).expect("reserve a port");
        let port = reservation.local_addr().expect("reserved address").port();
        drop(reservation);
        let (release_start, start_gate) = mpsc::channel();
        let (pending, done) =
            spawn_pending_service(root.path().to_owned(), port, ready_capacity, start_gate);
        let is_running = Arc::clone(&pending.is_running);
        release_start.send(()).expect("start initialization");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !is_running.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::yield_now();
        }
        let reached_readiness = is_running.load(Ordering::Acquire);
        if reached_readiness && ready_capacity == 1 {
            assert!(pending
                .ready
                .recv_timeout(Duration::from_secs(5))
                .expect("readiness is queued")
                .is_ok());
        }
        pending.cancel();
        done.recv_timeout(Duration::from_secs(5))
            .expect("cancelled readiness exits");

        assert!(reached_readiness, "listener reaches readiness delivery");
        assert!(!is_running.load(Ordering::Acquire));
        let listener =
            TcpListener::bind(("127.0.0.1", port)).expect("cancelled listener is released");
        drop(listener);
    }
}
// //// /取消就绪交付后释放实际监听端口 ////
