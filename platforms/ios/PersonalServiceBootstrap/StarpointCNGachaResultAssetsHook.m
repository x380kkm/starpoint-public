// audience: internal
// # cn-gacha-result-assets-hook
// 装备结果场景通过当前卡池的资源组加载概率主表和兑换点数据.
// AIR 的 MethodFrame 连接当前调用链, 新数组和资源组由原生 AVM 构造器创建.
// 对象布局, 静态类槽位和方法签名对应 CN iOS 1.8.4.

#import <mach-o/dyld.h>

#include <stdatomic.h>

#import "StarpointCNAotRuntime.h"

enum {
    extraGroupsMethodIndex = 5401,
    equipmentResultSceneIndex = 97,
    sceneIndexOffset = 0x20,
    sceneParamsOffset = 0x30,
    objectVtableOffset = 0x10,
    methodInfoOffset = 0x10,
    methodImplementationOffset = 0x50,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    coreFrameOffset = 0x60,
    methodScopeOffset = 0x18,
    scopeAbcEnvironmentOffset = 0x10,
    abcScriptEnvironmentsOffset = 0x30,
    assetGroupScriptSlot = 0x5c30,
    scriptGlobalOffset = 0x20,
    gachaReleaseListMethodSlot = 0x138,
    arrayGetUintPropertySlot = 0x90,
};

typedef struct {
    void *previous;
    void *environment;
    uintptr_t defaultXmlNamespace;
} AvmMethodFrame;

typedef void *(*ExtraGroupsMethod)(void *, void *, void *, void *);
typedef void *(*GroupConstructor)(void *, void *, void *);
typedef uintptr_t (*ArrayElementGetter)(void *, uint32_t);
typedef void *(*ArrayConstructor)(AvmMethodFrame *, uint32_t, const uintptr_t *);
typedef void *(*ScriptInitializer)(void **, AvmMethodFrame *);
typedef void (*InterruptCheck)(void *);
typedef void (*NullObjectError)(void *);

static ExtraGroupsMethod originalExtraGroups;
static ArrayConstructor createArray;
static ScriptInitializer initializeScript;
static InterruptCheck checkInterrupt;
static NullObjectError raiseNullObjectError;

// //// 读取当前卡池资源组的静态类 [@x380kkm 2026-09-20] ////
static void *readPointer(void *object, size_t offset) {
    return *(void **)((uint8_t *)object + offset);
}

static void *assetGroupClass(void *environment, AvmMethodFrame *frame) {
    void *scope = readPointer(environment, methodScopeOffset);
    void *abcEnvironment = readPointer(scope, scopeAbcEnvironmentOffset);
    void *scriptEnvironments = readPointer(abcEnvironment, abcScriptEnvironmentsOffset);
    void **slot = (void **)((uint8_t *)scriptEnvironments + assetGroupScriptSlot);
    void *script = *slot;
    if (script == NULL) {
        script = initializeScript(slot, frame);
    }
    return readPointer(script, scriptGlobalOffset);
}
// //// /读取当前卡池资源组的静态类 ////

// //// 使用原生构造器声明单个卡池的资源 [@x380kkm 2026-09-20] ////
static void *currentGachaGroups(void *scene, void *environment) {
    void *methodInfo = readPointer(environment, methodInfoOffset);
    void *pool = readPointer(methodInfo, methodPoolOffset);
    void *core = readPointer(pool, poolCoreOffset);
    void **frameSlot = (void **)((uint8_t *)core + coreFrameOffset);
    checkInterrupt(core);
    AvmMethodFrame frame = {*frameSlot, environment, 0};
    *frameSlot = &frame;

    void *parameters = readPointer(scene, sceneParamsOffset);
    void *groupClass = assetGroupClass(environment, &frame);
    if (parameters == NULL || groupClass == NULL) {
        raiseNullObjectError(environment);
        __builtin_unreachable();
    }
    void *nativeVtable = readPointer(parameters, 0);
    ArrayElementGetter getElement = (ArrayElementGetter)readPointer(nativeVtable, arrayGetUintPropertySlot);
    uintptr_t gachaId = getElement(parameters, 0);
    void *gachaIds = createArray(&frame, 1, &gachaId);
    void *vtable = readPointer(groupClass, objectVtableOffset);
    void *constructorEnvironment = readPointer(vtable, gachaReleaseListMethodSlot);
    void *constructorInfo = readPointer(constructorEnvironment, methodInfoOffset);
    GroupConstructor constructor = (GroupConstructor)readPointer(constructorInfo, methodImplementationOffset);
    void *group = constructor(groupClass, gachaIds, constructorEnvironment);
    uintptr_t groupAtom = (uintptr_t)group | 1;
    void *groups = createArray(&frame, 1, &groupAtom);

    checkInterrupt(core);
    *frameSlot = frame.previous;
    return groups;
}
// //// /使用原生构造器声明单个卡池的资源 ////

// //// 为装备结果场景补充当前卡池资源组 [@x380kkm 2026-09-20] ////
static void *extraGroups(void *receiver, void *previous, void *scene, void *environment) {
    if (scene != NULL && *(int32_t *)((uint8_t *)scene + sceneIndexOffset) == equipmentResultSceneIndex) {
        return currentGachaGroups(scene, environment);
    }
    return originalExtraGroups(receiver, previous, scene, environment);
}
// //// /为装备结果场景补充当前卡池资源组 ////

// //// 安装与客户端版本匹配的资源组钩子 [@x380kkm 2026-09-20] ////
__attribute__((constructor)) static void installCnGachaResultAssetsHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {extraGroupsMethodIndex, 0x103df91f8},
        {5349, 0x103df5fc0},
        {26724, 0x104baf8f4},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    createArray = (ArrayConstructor)(0x100a2735c + slide);
    initializeScript = (ScriptInitializer)(0x10050a8e8 + slide);
    checkInterrupt = (InterruptCheck)(0x100a26814 + slide);
    raiseNullObjectError = (NullObjectError)(0x100a2676c + slide);
    originalExtraGroups = (ExtraGroupsMethod)table[extraGroupsMethodIndex];
    table[extraGroupsMethodIndex] = (uintptr_t)extraGroups;
    atomic_thread_fence(memory_order_release);
}
// //// /安装与客户端版本匹配的资源组钩子 ////
