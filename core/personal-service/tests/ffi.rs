// audience: internal
// # personal-service-ffi
//
// 这些测试验证移动端调用的 C ABI 生命周期, 管理 token 和空指针处理.

use starpoint_personal_service::{
    starpoint_personal_service_copy_management_token, starpoint_personal_service_flush,
    starpoint_personal_service_is_running, starpoint_personal_service_port,
    starpoint_personal_service_start, starpoint_personal_service_start_with_cdn_root,
    starpoint_personal_service_stop,
};
use std::ffi::{CStr, CString};
use std::fs;
use std::ptr;
use tempfile::TempDir;

mod support;

use support::request;

// //// 通过 C ABI 控制服务并读取管理 token [@x380kkm 2026-07-23] ////
#[test]
fn starts_flushes_and_stops_through_the_c_abi() {
    let root = TempDir::new().expect("temporary service directory is created");
    let root_path =
        CString::new(root.path().to_string_lossy().as_bytes()).expect("path has no NUL");

    let service = unsafe { starpoint_personal_service_start(root_path.as_ptr(), 0) };
    assert!(!service.is_null());
    assert_ne!(unsafe { starpoint_personal_service_port(service) }, 0);
    assert_eq!(unsafe { starpoint_personal_service_is_running(service) }, 1);
    let token_length =
        unsafe { starpoint_personal_service_copy_management_token(service, ptr::null_mut(), 0) };
    let mut short_buffer = [b'x' as i8; 4];
    assert_eq!(
        unsafe {
            starpoint_personal_service_copy_management_token(
                service,
                short_buffer.as_mut_ptr(),
                short_buffer.len(),
            )
        },
        token_length,
    );
    assert_eq!(short_buffer, [b'x' as i8; 4]);
    let mut token = vec![0_i8; token_length];
    assert_eq!(
        unsafe {
            starpoint_personal_service_copy_management_token(
                service,
                token.as_mut_ptr(),
                token.len(),
            )
        },
        token_length,
    );
    let token = unsafe { CStr::from_ptr(token.as_ptr()) }
        .to_str()
        .expect("management token is UTF-8");
    assert_eq!(token.len(), 43);
    assert!(token
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
    assert_eq!(unsafe { starpoint_personal_service_flush(service) }, 0);
    unsafe { starpoint_personal_service_stop(service) };
}
// //// /通过 C ABI 控制服务并读取管理 token ////

// //// 通过 C ABI 传入显式 CN CDN 根目录 [@x380kkm 2026-08-11] ////
#[test]
fn starts_with_explicit_cdn_root_through_the_c_abi() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("temporary CDN directory is created");
    let asset_path = cdn_root.path().join("EntityLists").join("Custom.csv");
    fs::create_dir_all(asset_path.parent().expect("asset parent exists"))
        .expect("asset directory is created");
    fs::write(&asset_path, "Id,Entity\r\n8,FFI\r\n").expect("asset is written");
    let root_path =
        CString::new(root.path().to_string_lossy().as_bytes()).expect("path has no NUL");
    let cdn_path =
        CString::new(cdn_root.path().to_string_lossy().as_bytes()).expect("path has no NUL");

    let service = unsafe {
        starpoint_personal_service_start_with_cdn_root(root_path.as_ptr(), cdn_path.as_ptr(), 0)
    };
    assert!(!service.is_null());
    let response = request(
        unsafe { starpoint_personal_service_port(service) },
        "GET",
        "/patch/cn/EntityLists/Custom.csv",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("Id,Entity\r\n8,FFI\r\n"));
    unsafe { starpoint_personal_service_stop(service) };
}
// //// /通过 C ABI 传入显式 CN CDN 根目录 ////

// //// 拒绝无效 C ABI 句柄 [@x380kkm 2026-07-22] ////
#[test]
fn rejects_null_c_abi_arguments() {
    let valid_path = CString::new("valid-path").expect("literal has no NUL");
    let empty_path = CString::new("").expect("literal has no NUL");
    assert!(unsafe { starpoint_personal_service_start(ptr::null(), 0) }.is_null());
    assert!(
        unsafe { starpoint_personal_service_start_with_cdn_root(ptr::null(), ptr::null(), 0) }
            .is_null()
    );
    assert!(unsafe {
        starpoint_personal_service_start_with_cdn_root(valid_path.as_ptr(), ptr::null(), 0)
    }
    .is_null());
    assert!(unsafe {
        starpoint_personal_service_start_with_cdn_root(ptr::null(), valid_path.as_ptr(), 0)
    }
    .is_null());
    assert!(unsafe {
        starpoint_personal_service_start_with_cdn_root(valid_path.as_ptr(), empty_path.as_ptr(), 0)
    }
    .is_null());
    assert_eq!(unsafe { starpoint_personal_service_port(ptr::null()) }, 0);
    assert_eq!(
        unsafe { starpoint_personal_service_is_running(ptr::null()) },
        0
    );
    assert_eq!(unsafe { starpoint_personal_service_flush(ptr::null()) }, -1);
    assert_eq!(
        unsafe {
            starpoint_personal_service_copy_management_token(ptr::null(), ptr::null_mut(), 0)
        },
        0,
    );
    unsafe { starpoint_personal_service_stop(ptr::null_mut()) };
}
// //// /拒绝无效 C ABI 句柄 ////
