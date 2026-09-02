// audience: internal
// # personal-service-cn-reference-route-coverage-tests
//
// 该文件以 audit-cn-reference-route-coverage.mjs 生成的路由清单验证 CN 请求不会落入严格 404.

#[allow(dead_code)]
mod support;

use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

const REFERENCE_ROUTES: &str = include_str!("fixtures/cn_reference_routes.txt");
const MALFORMED_MESSAGEPACK: &[u8] = b"wQ==";

// //// 验证参考 CN 游戏路由均被个人服务识别 [@x380kkm 2026-08-22] ////
#[test]
fn reference_game_routes_do_not_fall_through_to_strict_not_found() {
    let routes = REFERENCE_ROUTES
        .lines()
        .filter(|route| !route.trim().is_empty())
        .map(|route| {
            route
                .split_once(' ')
                .expect("fixture route contains method and path")
        })
        .collect::<Vec<_>>();
    assert!(!routes.is_empty(), "fixture contains reference routes");

    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let mut missing = Vec::new();
    for (method, path) in routes {
        let is_not_found = match method {
            "POST" => support::request_with_body(
                service.port(),
                method,
                path,
                "application/x-www-form-urlencoded",
                MALFORMED_MESSAGEPACK,
            )
            .starts_with("HTTP/1.1 404 Not Found"),
            "GET" => support::request_bytes(service.port(), method, path)
                .starts_with(b"HTTP/1.1 404 Not Found"),
            unsupported => panic!("unsupported fixture method: {unsupported}"),
        };
        if is_not_found {
            missing.push(format!("{method} {path}"));
        }
    }
    service.stop().expect("service stops cleanly");
    assert!(
        missing.is_empty(),
        "reference routes fell through strict 404:\n{}",
        missing.join("\n")
    );
}
// //// /验证参考 CN 游戏路由均被个人服务识别 ////
