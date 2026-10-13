// audience: internal
// # cn-local-product-tests
// 此测试核对商品方法表, 枚举首次初始化, 缓存读取和 AIR 调用帧恢复.

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNLocalProductHook.m"

static uint8_t environment[0x20];
static uint8_t methodInfo[0x38];
static uint8_t pool[0x10];
static uint8_t core[0x68];
static uint8_t scope[0x18];
static uint8_t abcEnvironment[0x38];
static uint8_t scripts[buyableStateScriptSlot + sizeof(void *)];
static uint8_t script[0x28];
static uint8_t stateClass[0x78];
_Alignas(void *) static uint8_t possibleState;
static uint8_t previousFrame;
static size_t initializations;
static size_t interrupts;
static uint32_t canValidateProductMethods;
static uintptr_t productMethods[buyableMethodIndex + 1] = {
    [withinPeriodMethodIndex] = 0x104a664f0,
    [displayableMethodIndex] = 0x104a65000,
    [buyableMethodIndex] = 0x104a61024,
};

// //// 校验测试方法表中的原生函数地址 [@x380kkm 2026-09-26] ////
uintptr_t *starpointCnValidatedMethodTable(const StarpointCnMethodSignature *signatures, size_t count) {
    if (!canValidateProductMethods) {
        return NULL;
    }
    for (size_t index = 0; index < count; index += 1) {
        assert(productMethods[signatures[index].index] == signatures[index].address);
    }
    return productMethods;
}

static void writePointer(void *object, size_t offset, void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

// //// 提供可观察的 AIR 初始化和中断边界 [@x380kkm 2026-09-22] ////
static void *loadState(void **slot, AvmMethodFrame *frame) {
    assert(slot == (void **)(scripts + buyableStateScriptSlot));
    assert(frame->previous == &previousFrame && frame->environment == environment);
    assert(readPointer(core, coreFrameOffset) == frame);
    initializations += 1;
    *slot = script;
    return script;
}

static void checkStateInterrupt(void *receiver) {
    assert(receiver == core);
    interrupts += 1;
}

static void unexpectedNull(void *receiver) {
    (void)receiver;
    abort();
}
// //// /提供可观察的 AIR 初始化和中断边界 ////

// //// 保留日期方法并连接本地商品状态方法 [@x380kkm 2026-09-26] ////
static void verifyInstalledProductMethods(void) {
    canValidateProductMethods = 1;
    installCnLocalProductHook();
    assert(productMethods[withinPeriodMethodIndex] == 0x104a664f0);
    assert(productMethods[displayableMethodIndex] == (uintptr_t)localProductDisplayable);
    assert(productMethods[buyableMethodIndex] == (uintptr_t)localProductBuyable);
    assert(localProductDisplayable(NULL, NULL, 0, NULL) == 1);
}

// //// 保留枚举身份并恢复两次调用的外层帧 [@x380kkm 2026-09-22] ////
int main(void) {
    verifyInstalledProductMethods();
    writePointer(environment, methodInfoOffset, methodInfo);
    writePointer(environment, methodScopeOffset, scope);
    writePointer(methodInfo, methodPoolOffset, pool);
    writePointer(pool, poolCoreOffset, core);
    writePointer(core, coreFrameOffset, &previousFrame);
    writePointer(scope, scopeAbcEnvironmentOffset, abcEnvironment);
    writePointer(abcEnvironment, abcScriptEnvironmentsOffset, scripts);
    writePointer(script, scriptGlobalOffset, stateClass);
    writePointer(stateClass, possibleStateOffset, &possibleState);
    initializeScript = loadState;
    checkInterrupt = checkStateInterrupt;
    raiseNullObjectError = unexpectedNull;

    for (size_t call = 0; call < 2; call += 1) {
        assert(localProductBuyable(NULL, NULL, NULL, 0, NULL, environment) == &possibleState);
        assert(readPointer(core, coreFrameOffset) == &previousFrame);
    }
    assert(initializations == 1 && interrupts == 4);
    puts("日期方法保留原生地址, 商品状态方法连接成功, 枚举初始化一次且调用帧恢复.");
    return 0;
}
