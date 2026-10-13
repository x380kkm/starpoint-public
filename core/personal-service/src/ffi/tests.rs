// audience: internal
// # service-start-error-tests
//
// 测试使用实际端口冲突验证 C ABI 错误详情, 线程隔离和成功启动后的清理.

use super::*;
use std::ffi::CString;
use std::net::TcpListener;
use std::thread;

// //// 读取 C ABI 最近一次启动错误 [@x380kkm 2026-09-10] ////
fn last_start_error() -> Option<String> {
    unsafe {
        let required = starpoint_personal_service_copy_last_start_error(ptr::null_mut(), 0);
        if required == 0 {
            return None;
        }
        let mut bytes = vec![0_u8; required];
        assert_eq!(
            starpoint_personal_service_copy_last_start_error(bytes.as_mut_ptr().cast(), required),
            required
        );
        Some(
            CStr::from_bytes_with_nul(&bytes)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned(),
        )
    }
}
// //// /读取 C ABI 最近一次启动错误 ////

// //// 保留端口冲突详情并隔离各线程错误 [@x380kkm 2026-09-10] ////
#[test]
fn start_error_is_thread_local_and_cleared_after_success() {
    let root = tempfile::tempdir().expect("temporary state directory");
    let path = CString::new(root.path().to_str().unwrap()).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("occupied loopback port");
    let port = listener.local_addr().unwrap().port();
    unsafe {
        assert!(starpoint_personal_service_start(path.as_ptr(), port).is_null());
    }
    let error = last_start_error().expect("startup reports its actual error");
    assert!(error.contains("failed to bind loopback listener"));
    thread::spawn(|| {
        assert!(last_start_error().is_none());
        unsafe {
            assert!(starpoint_personal_service_start(ptr::null(), 0).is_null());
        }
        assert_eq!(last_start_error().as_deref(), Some("service path is null"));
    })
    .join()
    .expect("independent error thread");
    assert_eq!(last_start_error(), Some(error));
    unsafe {
        let service = starpoint_personal_service_start(path.as_ptr(), 0);
        assert!(!service.is_null());
        assert!(last_start_error().is_none());
        starpoint_personal_service_stop(service);
    }
}
// //// /保留端口冲突详情并隔离各线程错误 ////
