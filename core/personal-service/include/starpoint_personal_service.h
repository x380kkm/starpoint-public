// audience: external
// # starpoint-personal-service
//
// 该接口控制同一应用进程内的个人服务并复制管理 token. 调用方持有返回句柄, 串行调用接口, 并在应用暂停前调用 flush.
// token 复制函数返回包含结尾 NUL 的所需长度. buffer 为空或不足时不写入.

#ifndef STARPOINT_PERSONAL_SERVICE_H
#define STARPOINT_PERSONAL_SERVICE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct StarpointPersonalService StarpointPersonalService;

// //// 控制个人服务并复制管理 token [@x380kkm 2026-07-23] ////
StarpointPersonalService *starpoint_personal_service_start(const char *root_path, uint16_t port);

StarpointPersonalService *starpoint_personal_service_start_with_cdn_root(
    const char *root_path,
    const char *cn_asset_root,
    uint16_t port
);

uint16_t starpoint_personal_service_port(const StarpointPersonalService *service);

int32_t starpoint_personal_service_is_running(const StarpointPersonalService *service);

size_t starpoint_personal_service_copy_management_token(
    const StarpointPersonalService *service,
    char *buffer,
    size_t buffer_length
);

int32_t starpoint_personal_service_flush(const StarpointPersonalService *service);

void starpoint_personal_service_stop(StarpointPersonalService *service);
// //// /控制个人服务并复制管理 token ////

#ifdef __cplusplus
}
#endif

#endif
