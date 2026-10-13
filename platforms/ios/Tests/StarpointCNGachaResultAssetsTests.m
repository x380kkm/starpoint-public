// audience: internal
// # cn-gacha-result-assets-tests
// 此测试验证装备结果场景加载请求池的数据, 并保留其他场景的资源组和 AIR 调用帧.

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNGachaResultAssetsHook.m"

typedef struct {
    void *vtable;
    uintptr_t item;
} TestArray;

static uint8_t scene[0x40];
static uint8_t methodEnvironment[0x20];
static uint8_t methodInfo[0x38];
static uint8_t pool[0x10];
static uint8_t core[0x70];
static uint8_t scope[0x18];
static uint8_t abcEnvironment[0x38];
static uint8_t scriptEnvironments[0x5c38];
static uint8_t script[0x28];
static uint8_t groupClass[0x18];
static uint8_t groupVtable[0x140];
static uint8_t constructorEnvironment[0x18];
static uint8_t constructorInfo[0x58];
static uint8_t arrayVtable[0x98];
static TestArray inputParameters;
static TestArray allocatedArrays[2];
static void *constructedGroup[1];
static size_t arrayCount;
static size_t originalCalls;
static size_t scriptInitializations;
static size_t interruptChecks;
static void *priorFrame = (void *)0x1230;

// //// 提供原生边界的可观察替身 [@x380kkm 2026-09-20] ////
uintptr_t *starpointCnValidatedMethodTable(const StarpointCnMethodSignature *signatures, size_t count) {
    (void)signatures;
    (void)count;
    return NULL;
}

static void writePointer(void *object, size_t offset, void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

static uintptr_t arrayElement(void *object, uint32_t index) {
    assert(object == &inputParameters && index == 0);
    return ((TestArray *)object)->item;
}

static void *newArray(AvmMethodFrame *frame, uint32_t count, const uintptr_t *items) {
    assert(frame->environment == methodEnvironment && frame->previous == priorFrame);
    assert(readPointer(core, coreFrameOffset) == frame);
    assert(count == 1 && arrayCount < 2);
    TestArray *array = &allocatedArrays[arrayCount++];
    array->item = items[0];
    return array;
}

static void *makeGroup(void *receiver, void *ids, void *environment) {
    assert(receiver == groupClass && environment == constructorEnvironment);
    constructedGroup[0] = ids;
    return constructedGroup;
}

static void *originalGroups(void *receiver, void *previous, void *current, void *environment) {
    assert(receiver == (void *)0x4560 && previous == (void *)0x7890);
    assert(current == scene || current == NULL);
    assert(environment == methodEnvironment);
    originalCalls += 1;
    return (void *)0xabc0;
}

static void *loadScript(void **slot, AvmMethodFrame *frame) {
    assert(frame->environment == methodEnvironment);
    assert(slot == (void **)(scriptEnvironments + assetGroupScriptSlot));
    scriptInitializations += 1;
    *slot = script;
    return script;
}

static void interrupt(void *value) {
    assert(value == core);
    interruptChecks += 1;
}

static void nullError(void *environment) {
    (void)environment;
    abort();
}
// //// /提供原生边界的可观察替身 ////

// //// 构造客户端对象布局与静态资源组环境 [@x380kkm 2026-09-20] ////
static void prepare(void) {
    arrayCount = 0;
    interruptChecks = 0;
    createArray = newArray;
    originalExtraGroups = originalGroups;
    initializeScript = loadScript;
    checkInterrupt = interrupt;
    raiseNullObjectError = nullError;
    writePointer(scene, sceneParamsOffset, &inputParameters);
    writePointer(methodEnvironment, methodInfoOffset, methodInfo);
    writePointer(methodInfo, methodPoolOffset, pool);
    writePointer(pool, poolCoreOffset, core);
    writePointer(core, coreFrameOffset, priorFrame);
    writePointer(methodEnvironment, methodScopeOffset, scope);
    writePointer(scope, scopeAbcEnvironmentOffset, abcEnvironment);
    writePointer(abcEnvironment, abcScriptEnvironmentsOffset, scriptEnvironments);
    writePointer(scriptEnvironments, assetGroupScriptSlot, script);
    writePointer(script, scriptGlobalOffset, groupClass);
    writePointer(groupClass, objectVtableOffset, groupVtable);
    writePointer(groupVtable, gachaReleaseListMethodSlot, constructorEnvironment);
    writePointer(constructorEnvironment, methodInfoOffset, constructorInfo);
    writePointer(constructorInfo, methodImplementationOffset, (void *)makeGroup);
    writePointer(arrayVtable, arrayGetUintPropertySlot, (void *)arrayElement);
    inputParameters.vtable = arrayVtable;
    inputParameters.item = (3 << 3) | 6;
}
// //// /构造客户端对象布局与静态资源组环境 ////

// //// 验证当前池加载和其他场景委托 [@x380kkm 2026-09-20] ////
int main(void) {
    prepare();
    *(int32_t *)(scene + sceneIndexOffset) = equipmentResultSceneIndex;
    TestArray *groups = extraGroups((void *)0x4560, (void *)0x7890, scene, methodEnvironment);
    assert(groups == &allocatedArrays[1] && arrayCount == 2 && originalCalls == 0);
    assert(groups->item == ((uintptr_t)constructedGroup | 1));
    assert(constructedGroup[0] == &allocatedArrays[0]);
    assert(allocatedArrays[0].item == inputParameters.item);
    assert(readPointer(core, coreFrameOffset) == priorFrame && interruptChecks == 2);

    prepare();
    writePointer(scriptEnvironments, assetGroupScriptSlot, NULL);
    inputParameters.item = (1514 << 3) | 6;
    extraGroups((void *)0x4560, (void *)0x7890, scene, methodEnvironment);
    assert(scriptInitializations == 1 && allocatedArrays[0].item == inputParameters.item);
    assert(readPointer(core, coreFrameOffset) == priorFrame);

    *(int32_t *)(scene + sceneIndexOffset) = 16;
    assert(extraGroups((void *)0x4560, (void *)0x7890, scene, methodEnvironment) == (void *)0xabc0);
    assert(extraGroups((void *)0x4560, (void *)0x7890, NULL, methodEnvironment) == (void *)0xabc0);
    assert(originalCalls == 2 && arrayCount == 2);
    puts("装备结果资源组保持请求池 ID, 其他场景保持原处理, AIR 调用帧完整恢复.");
    return 0;
}
// //// /验证当前池加载和其他场景委托 ////
