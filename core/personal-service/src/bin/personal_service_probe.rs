// audience: internal
// # personal-service-probe
//
// 该测试进程启动个人服务并保持运行. 测试可请求启动时的管理 token.

use starpoint_personal_service::PersonalService;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// //// 为本机测试托管个人服务 [@x380kkm 2026-07-27] ////
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let root_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("service root argument is required")?;
    let mut cn_asset_root = None;
    let mut report_management_token = false;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--report-management-token") if !report_management_token => {
                report_management_token = true;
            }
            Some("--cdn-root") if cn_asset_root.is_none() => {
                cn_asset_root = Some(PathBuf::from(
                    arguments.next().ok_or("--cdn-root requires a path")?,
                ));
            }
            _ => return Err("unsupported personal service probe argument".into()),
        }
    }
    let cn_asset_root = cn_asset_root.unwrap_or_else(|| root_path.join("cdn").join("cn"));
    let service = PersonalService::start_with_cdn_root(root_path, 0, cn_asset_root)?;
    if report_management_token {
        println!(
            "{{\"port\":{},\"managementToken\":\"{}\"}}",
            service.port(),
            service.management_token()
        );
    } else {
        println!("{}", service.port());
    }
    io::stdout().flush()?;
    loop {
        thread::sleep(Duration::from_secs(1));
        std::hint::black_box(&service);
    }
}
// //// /为本机测试托管个人服务 ////
