// audience: internal
// # cn-rescue-auto-start-hook
// 救援访客选择房间时启用当局续战, 恢复房间时保留传入状态.
// 方法地址, 对象布局和调用点对应 CN iOS 1.8.4. 动态属性写入由原入房方法执行.

#include <mach-o/dyld.h>
#include <stdatomic.h>

#include "StarpointCNAotRuntime.h"

enum {
    autoStartMethodIndex = 21161,
    enterRoomMethodIndex = 42187,
    methodInfoOffset = 0x10,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    poolNamesOffset = 0x108,
    coreFrameOffset = 0x60,
    serviceReasonOffset = 0x48,
    serviceContinuationOffset = 0x88,
    serviceConfigOffset = 0x90,
    configAttentionOffset = 0x58,
    enumIndexOffset = 0x20,
    isHostNameOffset = 0x518e8,
    isAutoStartNameOffset = 0xa0470,
    selectReasonIndex = 0,
    resumeReasonIndex = 1,
    someOptionIndex = 0,
};

typedef uint32_t (*AutoStartGetter)(void *, void *, void *);
typedef uintptr_t (*PropertyReader)(void *, const void *, uintptr_t);
typedef uint32_t (*BooleanConverter)(void *, uintptr_t);

static AutoStartGetter originalAutoStart;
static PropertyReader readProperty;
static BooleanConverter convertBoolean;
static uintptr_t enterRoomReturnAddress;

// //// 读取 AOT 对象指针与枚举值 [@x380kkm 2026-09-26] ////
static void *readPointer(const void *object, size_t offset) {
    return *(void *const *)((const uint8_t *)object + offset);
}

static int32_t enumIndex(const void *object) {
    return *(const int32_t *)((const uint8_t *)object + enumIndexOffset);
}
// //// /读取 AOT 对象指针与枚举值 ////

// //// 从当前入房调用帧读取救援访客的续战状态 [@x380kkm 2026-09-26] ////
__attribute__((used, noinline)) static uint32_t resolveRescueAutoStart(
    void *receiver, void *questId, void *environment, void *service, uintptr_t returnAddress
) {
    uint32_t configured = originalAutoStart(receiver, questId, environment);
    if (returnAddress != enterRoomReturnAddress || service == NULL) {
        return configured;
    }
    void *config = readPointer(service, serviceConfigOffset);
    void *reason = readPointer(service, serviceReasonOffset);
    if (config == NULL || reason == NULL) {
        return configured;
    }
    void *attention = readPointer(config, configAttentionOffset);
    if (attention == NULL || enumIndex(attention) != someOptionIndex) {
        return configured;
    }
    int32_t reasonIndex = enumIndex(reason);
    if (reasonIndex != selectReasonIndex && reasonIndex != resumeReasonIndex) {
        return configured;
    }
    uintptr_t continuation = *(const uintptr_t *)((uint8_t *)service + serviceContinuationOffset);
    if (continuation <= 4 || (continuation & 7) != 1) {
        return configured;
    }
    void *pool = readPointer(readPointer(environment, methodInfoOffset), methodPoolOffset);
    void *core = readPointer(pool, poolCoreOffset);
    void *frame = readPointer(core, coreFrameOffset);
    uint8_t *names = readPointer(pool, poolNamesOffset);
    if (frame == NULL || names == NULL) {
        return configured;
    }
    if (convertBoolean(frame, readProperty(frame, names + isHostNameOffset, continuation))) {
        return configured;
    }
    if (reasonIndex == selectReasonIndex) {
        return 1;
    }
    return convertBoolean(frame, readProperty(frame, names + isAutoStartNameOffset, continuation));
}
// //// /从当前入房调用帧读取救援访客的续战状态 ////

#if defined(__arm64__)
// //// 在原调用栈上传递入房对象和返回地址 [@x380kkm 2026-09-26] ////
__attribute__((naked)) static uint32_t rescueAutoStart(
    void *receiver __attribute__((unused)),
    void *questId __attribute__((unused)),
    void *environment __attribute__((unused))
) {
    __asm__ volatile(
        "mov x3, x20\n"
        "mov x4, x30\n"
        "b _resolveRescueAutoStart\n"
    );
}

// //// 连接方法表与版本匹配的入房调用点 [@x380kkm 2026-09-26] ////
__attribute__((constructor)) static void installCnRescueAutoStartHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {autoStartMethodIndex, 0x104a01e78},
        {enterRoomMethodIndex, 0x105391fc8},
        {42178, 0x105393d28},
        {49249, 0x102016df8},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    uintptr_t callAddress = 0x105392140 + slide;
    uint32_t callInstruction = *(const uint32_t *)callAddress;
    if (callInstruction != 0xd63f0100) {
        return;
    }
    enterRoomReturnAddress = callAddress + sizeof(callInstruction);
    originalAutoStart = (AutoStartGetter)table[autoStartMethodIndex];
    readProperty = (PropertyReader)(0x100a26eec + slide);
    convertBoolean = (BooleanConverter)(0x100a26a30 + slide);
    table[autoStartMethodIndex] = (uintptr_t)rescueAutoStart;
    atomic_thread_fence(memory_order_release);
}
// //// /连接方法表与版本匹配的入房调用点 ////
#endif
