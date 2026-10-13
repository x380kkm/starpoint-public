// audience: external
// # personal-service-ffi
//
// 该模块暴露移动端使用的 C ABI. 调用约束由 include/starpoint_personal_service.h 定义.
// 启动错误保存在调用线程中, 字符串复制接口返回包含结尾 NUL 的所需长度.

use crate::{PersonalService, PersonalServiceError};
use std::cell::RefCell;
use std::ffi::{c_char, CStr};
use std::path::PathBuf;
use std::ptr;

// //// 保存调用线程的启动错误 [@x380kkm 2026-09-10] ////
thread_local! {
    static LAST_START_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}
// //// /保存调用线程的启动错误 ////

#[repr(C)]
pub struct StarpointPersonalService {
    service: PersonalService,
}

// //// 复制 C 字符串路径 [@x380kkm 2026-08-11] ////
unsafe fn copy_path(path: *const c_char) -> Result<PathBuf, PersonalServiceError> {
    if path.is_null() {
        return Err(PersonalServiceError::new("service path is null"));
    }
    CStr::from_ptr(path)
        .to_str()
        .map(PathBuf::from)
        .map_err(|error| PersonalServiceError::new(format!("service path is not UTF-8: {error}")))
}
// //// /复制 C 字符串路径 ////

// //// 把个人服务结果转换为 C 句柄 [@x380kkm 2026-08-11] ////
fn into_handle(
    service: Result<PersonalService, PersonalServiceError>,
) -> *mut StarpointPersonalService {
    LAST_START_ERROR.with(|last_error| match service {
        Ok(service) => {
            last_error.borrow_mut().clear();
            Box::into_raw(Box::new(StarpointPersonalService { service }))
        }
        Err(error) => {
            *last_error.borrow_mut() = error.to_string();
            ptr::null_mut()
        }
    })
}
// //// /把个人服务结果转换为 C 句柄 ////

// //// 暴露移动端生命周期和管理 token 接口 [@x380kkm 2026-07-23] ////
/// # Safety
///
/// 非空 `root_path` 指向有效且以 NUL 结尾的字符串, 并在调用期间保持可读.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_start(
    root_path: *const c_char,
    port: u16,
) -> *mut StarpointPersonalService {
    into_handle(copy_path(root_path).and_then(|root_path| PersonalService::start(root_path, port)))
}

/// # Safety
///
/// `root_path` 和 `cn_asset_root` 指向有效且以 NUL 结尾的字符串, 并在调用期间保持可读.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_start_with_cdn_root(
    root_path: *const c_char,
    cn_asset_root: *const c_char,
    port: u16,
) -> *mut StarpointPersonalService {
    into_handle(copy_path(root_path).and_then(|root_path| {
        copy_path(cn_asset_root).and_then(|cn_asset_root| {
            PersonalService::start_with_cdn_root(root_path, port, cn_asset_root)
        })
    }))
}

// //// 复制当前线程最近一次启动错误 [@x380kkm 2026-09-10] ////
/// 返回当前线程最近一次启动失败的 UTF-8 消息长度, 包含结尾 NUL.
/// 当前线程最近一次启动成功时返回 0.
///
/// # Safety
///
/// 非空 `buffer` 在调用期间可写入 `buffer_length` 字节. 缓冲区不足时保持原内容.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_copy_last_start_error(
    buffer: *mut c_char,
    buffer_length: usize,
) -> usize {
    LAST_START_ERROR.with(|last_error| {
        let last_error = last_error.borrow();
        if last_error.is_empty() {
            return 0;
        }
        let bytes = last_error.as_bytes();
        let required_length = bytes.len() + 1;
        if !buffer.is_null() && buffer_length >= required_length {
            ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast::<u8>(), bytes.len());
            *buffer.add(bytes.len()) = 0;
        }
        required_length
    })
}
// //// /复制当前线程最近一次启动错误 ////

/// # Safety
///
/// 非空 `service` 来自任一 start 函数, 且尚未传给 stop.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_port(
    service: *const StarpointPersonalService,
) -> u16 {
    service.as_ref().map_or(0, |service| service.service.port())
}

/// # Safety
///
/// 非空 `service` 来自任一 start 函数, 且尚未传给 stop.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_is_running(
    service: *const StarpointPersonalService,
) -> i32 {
    service
        .as_ref()
        .map_or(0, |service| i32::from(service.service.is_running()))
}

/// # Safety
///
/// 非空 `service` 来自任一 start 函数. 非空 `buffer` 在调用期间可写入
/// `buffer_length` 字节. 返回值包含结尾 NUL, 缓冲区不足时不写入.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_copy_management_token(
    service: *const StarpointPersonalService,
    buffer: *mut c_char,
    buffer_length: usize,
) -> usize {
    let Some(service) = service.as_ref() else {
        return 0;
    };
    let token = service.service.management_token().as_bytes();
    let required_length = token.len() + 1;
    if buffer.is_null() || buffer_length < required_length {
        return required_length;
    }
    ptr::copy_nonoverlapping(token.as_ptr(), buffer.cast::<u8>(), token.len());
    *buffer.add(token.len()) = 0;
    required_length
}

/// # Safety
///
/// 非空 `service` 来自任一 start 函数, 且尚未传给 stop.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_flush(
    service: *const StarpointPersonalService,
) -> i32 {
    match service.as_ref() {
        Some(service) if service.service.flush().is_ok() => 0,
        _ => -1,
    }
}

/// # Safety
///
/// 非空 `service` 来自任一 start 函数, 且只能传入一次.
#[no_mangle]
pub unsafe extern "C" fn starpoint_personal_service_stop(service: *mut StarpointPersonalService) {
    if !service.is_null() {
        let service = Box::from_raw(service);
        let _ = service.service.stop();
    }
}
// //// /暴露移动端生命周期和管理 token 接口 ////

#[cfg(test)]
mod tests;
