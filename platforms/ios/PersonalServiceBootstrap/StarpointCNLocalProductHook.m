// audience: internal
// # cn-local-product-hook
// 本地商品领取状态使用原客户端的 Possible 枚举.
// AIR 调用帧和静态枚举的惰性初始化遵循 CN iOS 1.8.4 的 AOT 布局.

#import <mach-o/dyld.h>
#include <stdatomic.h>

#import "StarpointCNAotRuntime.h"

enum {
    withinPeriodMethodIndex = 22244,
    displayableMethodIndex = 22258,
    buyableMethodIndex = 22286,
    methodInfoOffset = 0x10,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    coreFrameOffset = 0x60,
    methodScopeOffset = 0x18,
    scopeAbcEnvironmentOffset = 0x10,
    abcScriptEnvironmentsOffset = 0x30,
    buyableStateScriptSlot = 0x273c0,
    scriptGlobalOffset = 0x20,
    possibleStateOffset = 0x70,
};

typedef struct {
    void *previous;
    void *environment;
    uintptr_t defaultXmlNamespace;
} AvmMethodFrame;

typedef void *(*ScriptInitializer)(void **, AvmMethodFrame *);
typedef void (*InterruptCheck)(void *);
typedef void (*NullObjectError)(void *);

static ScriptInitializer initializeScript;
static InterruptCheck checkInterrupt;
static NullObjectError raiseNullObjectError;

static void *readPointer(void *object, size_t offset) {
    return *(void **)((uint8_t *)object + offset);
}

// //// 在 AIR 调用帧中读取原生可领取枚举 [@x380kkm 2026-09-22] ////
static void *localProductBuyable(void *receiver, void *player, void *config, double time, void *premium, void *environment) {
    (void)receiver;
    (void)player;
    (void)config;
    (void)time;
    (void)premium;
    void *methodInfo = readPointer(environment, methodInfoOffset);
    void *pool = readPointer(methodInfo, methodPoolOffset);
    void *core = readPointer(pool, poolCoreOffset);
    void **frameSlot = (void **)((uint8_t *)core + coreFrameOffset);
    checkInterrupt(core);
    AvmMethodFrame frame = {*frameSlot, environment, 0};
    *frameSlot = &frame;

    void *scope = readPointer(environment, methodScopeOffset);
    void *abcEnvironment = readPointer(scope, scopeAbcEnvironmentOffset);
    void *scripts = readPointer(abcEnvironment, abcScriptEnvironmentsOffset);
    void **slot = (void **)((uint8_t *)scripts + buyableStateScriptSlot);
    void *script = *slot;
    if (script == NULL) {
        script = initializeScript(slot, &frame);
    }
    void *stateClass = readPointer(script, scriptGlobalOffset);
    void *possible = stateClass == NULL ? NULL : readPointer(stateClass, possibleStateOffset);
    if (possible == NULL) {
        raiseNullObjectError(environment);
        __builtin_unreachable();
    }
    checkInterrupt(core);
    *frameSlot = frame.previous;
    return possible;
}
// //// /在 AIR 调用帧中读取原生可领取枚举 ////

// //// 返回本地商品的展示状态 [@x380kkm 2026-09-26] ////
static uint32_t localProductDisplayable(void *receiver, void *player, uint32_t showLimited, void *environment) {
    (void)receiver;
    (void)player;
    (void)showLimited;
    (void)environment;
    return 1;
}
// //// /返回本地商品的展示状态 ////

// //// 连接版本匹配的商品状态方法 [@x380kkm 2026-09-22] ////
__attribute__((constructor)) static void installCnLocalProductHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {withinPeriodMethodIndex, 0x104a664f0},
        {displayableMethodIndex, 0x104a65000},
        {buyableMethodIndex, 0x104a61024},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    initializeScript = (ScriptInitializer)(0x10050a8e8 + slide);
    checkInterrupt = (InterruptCheck)(0x100a26814 + slide);
    raiseNullObjectError = (NullObjectError)(0x100a2676c + slide);
    table[displayableMethodIndex] = (uintptr_t)localProductDisplayable;
    table[buyableMethodIndex] = (uintptr_t)localProductBuyable;
    atomic_thread_fence(memory_order_release);
}
