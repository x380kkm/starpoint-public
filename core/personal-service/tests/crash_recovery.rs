// audience: internal
// # personal-service-crash-recovery
//
// 此测试强制终止服务进程, 再验证 SQLite 自动恢复最后一次已提交的状态.

use starpoint_personal_service::PersonalService;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use tempfile::TempDir;

mod support;

use support::request;

struct ChildProcess {
    child: Child,
}

impl ChildProcess {
    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

// //// 验证强制终止后的 SQLite WAL 恢复 [@x380kkm 2026-07-22] ////
#[test]
fn restores_committed_state_after_process_termination() {
    let root = TempDir::new().expect("temporary service directory is created");
    let mut child = ChildProcess {
        child: Command::new(env!("CARGO_BIN_EXE_personal-service-probe"))
            .arg(root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .expect("probe process starts"),
    };
    let stdout = child.child.stdout.take().expect("probe stdout is captured");
    let mut output = BufReader::new(stdout);
    let mut port_line = String::new();
    output
        .read_line(&mut port_line)
        .expect("probe reports its port");
    let port = port_line
        .trim()
        .parse::<u16>()
        .expect("probe port is valid");

    let changed = request(port, "POST", "/v1/state/increment");
    assert!(changed.ends_with("{\"generation\":1}"));
    child.kill_and_wait();

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts after kill");
    let restored = request(restarted.port(), "GET", "/health");
    assert!(restored.ends_with("{\"status\":\"ok\",\"generation\":1}"));
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证强制终止后的 SQLite WAL 恢复 ////
