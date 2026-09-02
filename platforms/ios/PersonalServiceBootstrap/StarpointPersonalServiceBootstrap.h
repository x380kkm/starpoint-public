// audience: external
// # starpoint-personal-service-bootstrap
//
// 该接口保留 Framework 链接, 并为同进程原生宿主提供端口, 状态, 生命周期和管理 token.
// token 复制函数返回包含结尾 NUL 的所需长度. buffer 为空或不足时不写入.
// 生命周期计数器只用于诊断宿主确认真实后台刷盘和前台恢复, 不保存凭据.
// 管理入口默认隐藏, 游戏适配层只传入当前标题场景是否可见.
// CN CDN 配置键为 STARPOINT_CN_CDN_BUNDLE_PATH 或 StarpointCNCDNBundlePath.
// StarpointCNCDNBundleMode=direct 直接使用只读 App bundle, import 才复制到 Application Support.

#ifndef STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_H
#define STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum StarpointPersonalServiceBootstrapStartResult {
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_NOT_ATTEMPTED = 0,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SUCCEEDED = 1,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SERVICE_ROOT_FAILED = -1,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_CONFIGURATION_FAILED = -2,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_MISSING = -3,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_EMPTY = -4,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED = -5,
    STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SERVICE_FAILED = -6,
} StarpointPersonalServiceBootstrapStartResult;

// //// 提供 iOS 宿主使用的个人服务控制接口 [@x380kkm 2026-07-24] ////
void starpoint_personal_service_bootstrap_link(void);

void starpoint_personal_service_bootstrap_start(void);

uint16_t starpoint_personal_service_bootstrap_port(void);

int32_t starpoint_personal_service_bootstrap_is_running(void);

StarpointPersonalServiceBootstrapStartResult
starpoint_personal_service_bootstrap_last_start_result(void);

uint64_t starpoint_personal_service_bootstrap_cdn_import_count(void);

uint64_t starpoint_personal_service_bootstrap_background_flush_count(void);

int32_t starpoint_personal_service_bootstrap_last_background_flush_result(void);

uint64_t starpoint_personal_service_bootstrap_foreground_resume_count(void);

void starpoint_personal_service_bootstrap_set_management_entry_visible(int32_t visible);

int32_t starpoint_personal_service_bootstrap_management_entry_visible(void);

size_t starpoint_personal_service_bootstrap_copy_management_token(
    char *buffer,
    size_t buffer_length
);

int32_t starpoint_personal_service_bootstrap_flush(void);

void starpoint_personal_service_bootstrap_stop(void);
// //// /提供 iOS 宿主使用的个人服务控制接口 ////

#ifdef __cplusplus
}
#endif

#endif
