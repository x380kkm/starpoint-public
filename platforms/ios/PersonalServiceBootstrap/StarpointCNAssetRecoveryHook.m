// audience: internal
// # cn-asset-recovery-hook
// 资源恢复由成功回调提交完成记录, 销毁流程保留待恢复列表和原有清理行为.
// 恢复进度对象按登记顺序执行完成回调, 元数据提交先于父级场景通知.
// 方法地址, 调用点和 AIR 方法帧布局对应 CN iOS 1.8.4. 完成状态取自当前调用帧.

#include <mach-o/dyld.h>
#include <stdbool.h>
#include <stdatomic.h>

#include "StarpointCNAotRuntime.h"

enum {
    waitConstructorIndex = 5461,
    waitAddBehaviorMethodIndex = 5463,
    progressConstructorIndex = 28280,
    writeInfoMethodIndex = 30199,
    recoveryRunMethodIndex = 41653,
    recoveryDisposeMethodIndex = 41657,
    recoveryCompleteMethodIndex = 41658,
    objectVtableOffset = 0x10,
    recoveryGearOffset = 0x20,
    beforeDisposeMethodSlot = 0x188,
    methodInfoOffset = 0x10,
    methodImplementationOffset = 0x50,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    coreFrameOffset = 0x60,
    poolNamesOffset = 0x108,
    methodScopeOffset = 0x18,
    scopeAbcEnvironmentOffset = 0x10,
    abcScriptEnvironmentsOffset = 0x30,
    behaviorScriptSlot = 0x310,
    scriptGlobalOffset = 0x20,
    addTailNameOffset = 0x918,
    undefinedAtom = 4,
};

typedef struct AvmMethodFrame {
    struct AvmMethodFrame *previous;
    void *environment;
    uintptr_t defaultXmlNamespace;
} AvmMethodFrame;

typedef uintptr_t (*RecoveryCompleteMethod)(void *, void *);
typedef uintptr_t (*WriteInfoMethod)(void *, void *, void *);
typedef uint32_t (*GearPredicate)(void *, void *);
typedef void *(*WaitAddBehaviorMethod)(void *, void *);
typedef uintptr_t (*PropertyReader)(AvmMethodFrame *, const void *, uintptr_t);

static RecoveryCompleteMethod originalRecoveryComplete;
static WriteInfoMethod originalWriteInfo;
static uintptr_t recoveryWriteReturnAddress;
static WaitAddBehaviorMethod originalWaitAddBehavior;
static PropertyReader readProperty;
static uintptr_t recoveryConstructionMethods[3];

// //// 读取恢复对象和 AIR 方法环境 [@x380kkm 2026-09-27] ////
static void *readPointer(const void *object, size_t offset) {
    return *(void *const *)((const uint8_t *)object + offset);
}

static void *methodImplementation(void *environment) {
    return readPointer(readPointer(environment, methodInfoOffset), methodImplementationOffset);
}

static void *methodPool(void *environment) {
    return readPointer(readPointer(environment, methodInfoOffset), methodPoolOffset);
}

static AvmMethodFrame *methodFrame(void *environment) {
    return readPointer(readPointer(methodPool(environment), poolCoreOffset), coreFrameOffset);
}
// //// /读取恢复对象和 AIR 方法环境 ////

// //// 为恢复进度对象选择按登记顺序执行的完成行为 [@x380kkm 2026-09-27] ////
static bool hasRecoveryConstructionFrames(AvmMethodFrame *frame) {
    for (size_t index = 0; index < sizeof(recoveryConstructionMethods) / sizeof(recoveryConstructionMethods[0]); index += 1) {
        if (frame == NULL || methodImplementation(frame->environment) != (void *)recoveryConstructionMethods[index]) {
            return false;
        }
        frame = frame->previous;
    }
    return true;
}

__attribute__((used)) static void *getRecoveryCompletionAddBehavior(void *receiver, void *environment) {
    void *behavior = originalWaitAddBehavior(receiver, environment);
    AvmMethodFrame *frame = methodFrame(environment);
    if (!hasRecoveryConstructionFrames(frame)) {
        return behavior;
    }
    void *scope = readPointer(environment, methodScopeOffset);
    void *abc = readPointer(scope, scopeAbcEnvironmentOffset);
    void *scripts = readPointer(abc, abcScriptEnvironmentsOffset);
    void *script = readPointer(scripts, behaviorScriptSlot);
    void *behaviorClass = readPointer(script, scriptGlobalOffset);
    uint8_t *names = readPointer(methodPool(environment), poolNamesOffset);
    uintptr_t function = readProperty(frame, names + addTailNameOffset, (uintptr_t)behaviorClass + 1);
    return (void *)(function & ~(uintptr_t)7);
}
// //// /为恢复进度对象选择按登记顺序执行的完成行为 ////

// //// 仅让存活的恢复对象执行成功回调 [@x380kkm 2026-09-27] ////
__attribute__((used)) static uintptr_t completeAssetRecovery(void *receiver, void *environment) {
    void *gear = readPointer(receiver, recoveryGearOffset);
    if (gear != NULL) {
        void *predicate = readPointer(readPointer(gear, objectVtableOffset), beforeDisposeMethodSlot);
        if (!((GearPredicate)methodImplementation(predicate))(gear, predicate)) {
            return undefinedAtom;
        }
    }
    return originalRecoveryComplete(receiver, environment);
}

// //// 按直接成功调用帧提交恢复记录 [@x380kkm 2026-09-27] ////
static bool hasRecoveryCompletionCaller(void *environment) {
    AvmMethodFrame *frame = methodFrame(environment);
    return frame != NULL && frame->previous != NULL &&
        methodImplementation(frame->previous->environment) == (void *)completeAssetRecovery;
}

__attribute__((used, noinline)) static uintptr_t writeRecoveryInfoIfComplete(
    void *receiver, void *info, void *environment, uintptr_t returnAddress
) {
    if (returnAddress == recoveryWriteReturnAddress && !hasRecoveryCompletionCaller(environment)) {
        return undefinedAtom;
    }
    return originalWriteInfo(receiver, info, environment);
}
// //// /按直接成功调用帧提交恢复记录 ////

#if defined(__arm64__)
// //// 保留信息写入参数并传递原调用位置 [@x380kkm 2026-09-27] ////
__attribute__((naked)) static uintptr_t writeAssetInfo(
    void *receiver __attribute__((unused)),
    void *info __attribute__((unused)),
    void *environment __attribute__((unused))
) {
    __asm__ volatile(
        "mov x3, x30\n"
        "b _writeRecoveryInfoIfComplete\n"
    );
}

// //// 连接版本匹配的恢复回调和信息写入方法 [@x380kkm 2026-09-27] ////
__attribute__((constructor)) static void installCnAssetRecoveryHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {waitConstructorIndex, 0x103e9ad94},
        {waitAddBehaviorMethodIndex, 0x103e9ac9c},
        {progressConstructorIndex, 0x104c269d4},
        {writeInfoMethodIndex, 0x104f07b84},
        {recoveryRunMethodIndex, 0x10534bf50},
        {recoveryDisposeMethodIndex, 0x10534b2f8},
        {recoveryCompleteMethodIndex, 0x10534b274},
        {4070, 0x101f15a30},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    const uintptr_t calls[] = {0x10534b2cc, 0x10534b988, 0x103e9ae4c, 0x104c26c90, 0x10534c244};
    for (size_t index = 0; index < sizeof(calls) / sizeof(calls[0]); index += 1) {
        if (*(const uint32_t *)(calls[index] + slide) != 0xd63f0100) {
            return;
        }
    }
    recoveryWriteReturnAddress = calls[1] + slide + 4;
    originalRecoveryComplete = (RecoveryCompleteMethod)table[recoveryCompleteMethodIndex];
    originalWriteInfo = (WriteInfoMethod)table[writeInfoMethodIndex];
    originalWaitAddBehavior = (WaitAddBehaviorMethod)table[waitAddBehaviorMethodIndex];
    readProperty = (PropertyReader)(0x100a26eec + slide);
    recoveryConstructionMethods[0] = table[waitConstructorIndex];
    recoveryConstructionMethods[1] = table[progressConstructorIndex];
    recoveryConstructionMethods[2] = table[recoveryRunMethodIndex];
    table[recoveryCompleteMethodIndex] = (uintptr_t)completeAssetRecovery;
    table[writeInfoMethodIndex] = (uintptr_t)writeAssetInfo;
    table[waitAddBehaviorMethodIndex] = (uintptr_t)getRecoveryCompletionAddBehavior;
    atomic_thread_fence(memory_order_release);
}
// //// /连接版本匹配的恢复回调和信息写入方法 ////
#endif
