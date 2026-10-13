// audience: internal
// # cn-aot-runtime
// CN iOS 1.8.4 的方法表经过主程序 UUID, ABC 描述项和原函数地址验证后供适配层使用.

#ifndef STARPOINT_CN_AOT_RUNTIME_H
#define STARPOINT_CN_AOT_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    size_t index;
    uintptr_t address;
} StarpointCnMethodSignature;

uintptr_t *starpointCnValidatedMethodTable(
    const StarpointCnMethodSignature *signatures,
    size_t count
);

#endif
