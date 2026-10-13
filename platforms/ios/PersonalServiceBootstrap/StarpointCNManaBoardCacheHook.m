// audience: internal
// # cn-mana-board-cache-hook
// 玛纳节点和能力缓存共享当前可见节点范围. 范围变化时通过 AIR 的对象写屏障恢复 Lazy 的 None 状态.
// 对象布局, 方法环境和 Option 静态槽位对应 CN iOS 1.8.4 的 AOT 实现.

#include <mach-o/dyld.h>
#include <stdatomic.h>

#include "StarpointCNAotRuntime.h"

enum {
    maxManaNodesMethodIndex = 18318,
    maxAbilitiesMethodIndex = 18324,
    objectVtableOffset = 0x10,
    methodInfoOffset = 0x10,
    methodImplementationOffset = 0x50,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    coreFrameOffset = 0x60,
    coreCollectorOffset = 0xb8,
    methodScopeOffset = 0x18,
    scopeAbcEnvironmentOffset = 0x10,
    abcScriptEnvironmentsOffset = 0x30,
    optionScriptSlot = 0x290,
    scriptGlobalOffset = 0x20,
    optionNoneOffset = 0x38,
    maxManaNodesOffset = 0x98,
    maxAbilitiesOffset = 0xa0,
    lazyValueOffset = 0x20,
    maxManaNodesMethodSlot = 0x2d0,
    manaNodeIdsMethodSlot = 0x310,
    arrayLengthMethodSlot = 0xb8,
};

typedef struct {
    void *previous;
    void *environment;
    uintptr_t defaultXmlNamespace;
} AvmMethodFrame;

typedef void *(*ArrayGetter)(void *, void *);
typedef uint32_t (*ArrayLengthGetter)(void *, void *);
typedef void *(*ScriptInitializer)(void **, AvmMethodFrame *);
typedef void (*PointerWriter)(void *, void *, void **, void *);
typedef void (*InterruptCheck)(void *);
typedef void (*NullObjectError)(void *);

static ArrayGetter originalMaxManaNodes;
static ArrayGetter originalMaxAbilities;
static ScriptInitializer initializeScript;
static PointerWriter writeObjectPointer;
static InterruptCheck checkInterrupt;
static NullObjectError raiseNullObjectError;

// //// 从对象虚表读取方法环境和实现 [@x380kkm 2026-09-24] ////
static void *readPointer(void *object, size_t offset) {
    return *(void **)((uint8_t *)object + offset);
}

static void *methodEnvironment(void *receiver, size_t slot) {
    return readPointer(readPointer(receiver, objectVtableOffset), slot);
}

static void *methodImplementation(void *environment) {
    return readPointer(readPointer(environment, methodInfoOffset), methodImplementationOffset);
}

static uint32_t arrayLength(void *array) {
    void *environment = methodEnvironment(array, arrayLengthMethodSlot);
    return ((ArrayLengthGetter)methodImplementation(environment))(array, environment);
}
// //// /从对象虚表读取方法环境和实现 ////

// //// 通过静态枚举和写屏障同时清除节点与能力缓存 [@x380kkm 2026-09-24] ////
static void resetManaCaches(void *receiver, void *core, AvmMethodFrame *frame) {
    void *scope = readPointer(frame->environment, methodScopeOffset);
    void *abc = readPointer(scope, scopeAbcEnvironmentOffset);
    void *scripts = readPointer(abc, abcScriptEnvironmentsOffset);
    void **slot = (void **)((uint8_t *)scripts + optionScriptSlot);
    void *script = *slot;
    if (script == NULL) {
        script = initializeScript(slot, frame);
    }
    void *optionClass = readPointer(script, scriptGlobalOffset);
    void *none = optionClass == NULL ? NULL : readPointer(optionClass, optionNoneOffset);
    void *nodes = readPointer(receiver, maxManaNodesOffset);
    void *abilities = readPointer(receiver, maxAbilitiesOffset);
    if (none == NULL || nodes == NULL || abilities == NULL) {
        raiseNullObjectError(frame->environment);
        __builtin_unreachable();
    }
    void *collector = readPointer(core, coreCollectorOffset);
    writeObjectPointer(collector, nodes, (void **)((uint8_t *)nodes + lazyValueOffset), none);
    writeObjectPointer(collector, abilities, (void **)((uint8_t *)abilities + lazyValueOffset), none);
}
// //// /通过静态枚举和写屏障同时清除节点与能力缓存 ////

// //// 在可见节点数量变化时重建当前玛纳节点 [@x380kkm 2026-09-24] ////
static void *maxManaNodes(void *receiver, void *environment) {
    void *pool = readPointer(readPointer(environment, methodInfoOffset), methodPoolOffset);
    void *core = readPointer(pool, poolCoreOffset);
    void **frameSlot = (void **)((uint8_t *)core + coreFrameOffset);
    checkInterrupt(core);
    AvmMethodFrame frame = {*frameSlot, environment, 0};
    *frameSlot = &frame;

    void *nodes = originalMaxManaNodes(receiver, environment);
    void *idsEnvironment = methodEnvironment(receiver, manaNodeIdsMethodSlot);
    void *ids = ((ArrayGetter)methodImplementation(idsEnvironment))(receiver, idsEnvironment);
    if (nodes == NULL || ids == NULL) {
        raiseNullObjectError(environment);
        __builtin_unreachable();
    }
    if (arrayLength(nodes) != arrayLength(ids)) {
        resetManaCaches(receiver, core, &frame);
        nodes = originalMaxManaNodes(receiver, environment);
    }
    checkInterrupt(core);
    *frameSlot = frame.previous;
    return nodes;
}
// //// /在可见节点数量变化时重建当前玛纳节点 ////

// //// 读取能力前同步当前玛纳板范围 [@x380kkm 2026-09-24] ////
static void *maxAbilities(void *receiver, void *environment) {
    maxManaNodes(receiver, methodEnvironment(receiver, maxManaNodesMethodSlot));
    return originalMaxAbilities(receiver, environment);
}

// //// 连接版本匹配的玛纳板缓存方法 [@x380kkm 2026-09-24] ////
__attribute__((constructor)) static void installCnManaBoardCacheHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {maxManaNodesMethodIndex, 0x10496bf08},
        {maxAbilitiesMethodIndex, 0x10496b0a4},
        {18326, 0x10496aba0},
        {5261, 0x103deb9e4},
        {5263, 0x103deb884},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    initializeScript = (ScriptInitializer)(0x10050a8e8 + slide);
    writeObjectPointer = (PointerWriter)(0x10054a400 + slide);
    checkInterrupt = (InterruptCheck)(0x100a26814 + slide);
    raiseNullObjectError = (NullObjectError)(0x100a2676c + slide);
    originalMaxManaNodes = (ArrayGetter)table[maxManaNodesMethodIndex];
    originalMaxAbilities = (ArrayGetter)table[maxAbilitiesMethodIndex];
    table[maxManaNodesMethodIndex] = (uintptr_t)maxManaNodes;
    table[maxAbilitiesMethodIndex] = (uintptr_t)maxAbilities;
    atomic_thread_fence(memory_order_release);
}
// //// /连接版本匹配的玛纳板缓存方法 ////
