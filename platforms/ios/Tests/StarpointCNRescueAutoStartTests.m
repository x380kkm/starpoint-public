// audience: internal
// # cn-rescue-auto-start-tests
// 真实 AOT 布局和运行时边界替身覆盖入房, 恢复, 房主及动态属性异常.

#include <assert.h>
#include <setjmp.h>
#include <stdio.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNRescueAutoStartHook.m"

static uint8_t service[0x98], config[0x60], reason[0x28], attention[0x28];
static uint8_t environment[0x18], info[0x38], pool[0x110], core[0x68], names[0xa0480];
static uint32_t configured, isHost, isAutoStart, originalCalls, propertyCalls;
static int abortProperty;
static jmp_buf propertyException;
static void *const frame = (void *)0x2000;

// //// 提供可观察的 AOT 属性与配置读取 [@x380kkm 2026-09-26] ////
uintptr_t *starpointCnValidatedMethodTable(const StarpointCnMethodSignature *signatures, size_t count) {
    (void)signatures;
    (void)count;
    return NULL;
}

static void putPointer(void *object, size_t offset, void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

static void putIndex(void *object, int32_t value) {
    memcpy((uint8_t *)object + enumIndexOffset, &value, sizeof(value));
}

static uint32_t readConfigured(void *receiver, void *questId, void *methodEnvironment) {
    assert(receiver == (void *)0x3000 && questId == (void *)0x4000);
    assert(methodEnvironment == environment);
    originalCalls += 1;
    return configured;
}

static uintptr_t readDynamicProperty(void *methodFrame, const void *name, uintptr_t atom) {
    assert(methodFrame == frame && atom == 0x1001);
    propertyCalls += 1;
    if (abortProperty) {
        longjmp(propertyException, 1);
    }
    if (name == names + isHostNameOffset) {
        return 5 + ((uintptr_t)isHost << 3);
    }
    assert(name == names + isAutoStartNameOffset);
    return 5 + ((uintptr_t)isAutoStart << 3);
}

static uint32_t booleanValue(void *methodFrame, uintptr_t atom) {
    assert(methodFrame == frame && (atom == 5 || atom == 13));
    return atom == 13;
}

static uint32_t readRescue(void *callerService, uintptr_t returnAddress) {
    return resolveRescueAutoStart((void *)0x3000, (void *)0x4000, environment, callerService, returnAddress);
}
// //// /提供可观察的 AOT 属性与配置读取 ////

// //// 验证救援续战范围和异常后的原调用帧 [@x380kkm 2026-09-26] ////
int main(void) {
    originalAutoStart = readConfigured;
    readProperty = readDynamicProperty;
    convertBoolean = booleanValue;
    enterRoomReturnAddress = 0x105392144;
    putPointer(service, serviceConfigOffset, config);
    putPointer(service, serviceReasonOffset, reason);
    putPointer(service, serviceContinuationOffset, (void *)0x1001);
    putPointer(config, configAttentionOffset, attention);
    putPointer(environment, methodInfoOffset, info);
    putPointer(info, methodPoolOffset, pool);
    putPointer(pool, poolCoreOffset, core);
    putPointer(pool, poolNamesOffset, names);
    putPointer(core, coreFrameOffset, frame);

    assert(readRescue((void *)1, enterRoomReturnAddress + 4) == 0);
    assert(propertyCalls == 0 && originalCalls == 1);
    putIndex(attention, 1);
    assert(readRescue(service, enterRoomReturnAddress) == 0);
    configured = 1;
    assert(readRescue(service, enterRoomReturnAddress) == 1);
    assert(propertyCalls == 0);

    putIndex(attention, someOptionIndex);
    isHost = 1;
    configured = 0;
    assert(readRescue(service, enterRoomReturnAddress) == 0);
    isHost = 0;
    assert(readRescue(service, enterRoomReturnAddress) == 1);
    putIndex(reason, resumeReasonIndex);
    configured = 1;
    assert(readRescue(service, enterRoomReturnAddress) == 0);
    isAutoStart = 1;
    configured = 0;
    assert(readRescue(service, enterRoomReturnAddress) == 1);

    abortProperty = 1;
    if (setjmp(propertyException) == 0) {
        readRescue(service, enterRoomReturnAddress);
        assert(0);
    }
    abortProperty = 0;
    assert(readPointer(core, coreFrameOffset) == frame);
    assert(readRescue((void *)1, enterRoomReturnAddress + 4) == configured);
    isAutoStart = 0;
    assert(readRescue(service, enterRoomReturnAddress) == 0);
    puts("Rescue selection, continuation, host scope and exception frames verified.");
    return 0;
}
// //// /验证救援续战范围和异常后的原调用帧 ////
