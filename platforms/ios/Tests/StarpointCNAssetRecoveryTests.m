// audience: internal
// # cn-asset-recovery-tests
// AIR 调用帧替身覆盖成功提交, 失败与退出, 递归销毁和迟到回调.

#include <assert.h>
#include <setjmp.h>
#include <stdio.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNAssetRecoveryHook.m"

_Alignas(void *) static uint8_t receiver[0x28], gear[0x18], gearVtable[0x190];
_Alignas(void *) static uint8_t predicateEnvironment[0x18], predicateInfo[0x58];
_Alignas(void *) static uint8_t completeEnvironment[0x18], completeInfo[0x58];
_Alignas(void *) static uint8_t writeEnvironment[0x18], writeInfo[0x58], pool[0x110], core[0x68];
_Alignas(void *) static uint8_t disposalEnvironment[0x18], disposalInfo[0x58];
_Alignas(void *) static uint8_t behaviorEnvironment[0x20], behaviorInfo[0x58];
_Alignas(void *) static uint8_t scope[0x18], abc[0x38], scripts[0x318], behaviorScript[0x28], behaviorClass[0x20];
_Alignas(void *) static uint8_t names[0x930], constructionEnvironment[3][0x18], constructionInfo[3][0x58];
static uint8_t downloader, info;
static AvmMethodFrame outerFrame, completeFrame, disposeFrame, nestedFrame;
static AvmMethodFrame constructionFrames[3];
static uint32_t isActive;
static size_t completions, writes;
static bool failWrite;
static jmp_buf writeException;
static size_t behaviorReads;

// //// 构造恢复生命周期的方法环境 [@x380kkm 2026-09-27] ////
static void putPointer(void *object, size_t offset, const void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

static uint32_t beforeDispose(void *object, void *environment) {
    assert(object == gear && environment == predicateEnvironment);
    return isActive;
}

static void *defaultAddBehavior(void *object, void *environment) {
    assert(object == &downloader && environment == behaviorEnvironment);
    return (void *)0x1000;
}

static uintptr_t readAddTail(AvmMethodFrame *frame, const void *name, uintptr_t object) {
    assert(frame == &constructionFrames[0] && name == names + addTailNameOffset);
    assert(object == (uintptr_t)behaviorClass + 1);
    behaviorReads += 1;
    return 0x2001;
}

static uintptr_t writeInfoFile(void *object, void *data, void *environment) {
    assert(object == &downloader && data == &info && environment == writeEnvironment);
    if (failWrite) {
        putPointer(core, coreFrameOffset, &outerFrame);
        longjmp(writeException, 1);
    }
    writes += 1;
    return 20;
}

static uintptr_t writeRecoveryInfo(void) {
    return writeRecoveryInfoIfComplete(&downloader, &info, writeEnvironment, recoveryWriteReturnAddress);
}

static uintptr_t originalComplete(void *object, void *environment) {
    assert(object == receiver && environment == completeEnvironment);
    completions += 1;
    isActive = 0;
    completeFrame = (AvmMethodFrame){&outerFrame, completeEnvironment, 0};
    disposeFrame = (AvmMethodFrame){&completeFrame, disposalEnvironment, 0};
    nestedFrame = (AvmMethodFrame){&disposeFrame, disposalEnvironment, 0};
    putPointer(core, coreFrameOffset, &nestedFrame);
    assert(writeRecoveryInfo() == undefinedAtom);
    putPointer(core, coreFrameOffset, &disposeFrame);
    assert(writeRecoveryInfo() == 20);
    putPointer(core, coreFrameOffset, &outerFrame);
    return 12;
}

static void prepare(void) {
    putPointer(receiver, recoveryGearOffset, gear);
    putPointer(gear, objectVtableOffset, gearVtable);
    putPointer(gearVtable, beforeDisposeMethodSlot, predicateEnvironment);
    putPointer(predicateEnvironment, methodInfoOffset, predicateInfo);
    putPointer(predicateInfo, methodImplementationOffset, beforeDispose);
    putPointer(completeEnvironment, methodInfoOffset, completeInfo);
    putPointer(completeInfo, methodImplementationOffset, completeAssetRecovery);
    putPointer(writeEnvironment, methodInfoOffset, writeInfo);
    putPointer(writeInfo, methodPoolOffset, pool);
    putPointer(pool, poolCoreOffset, core);
    putPointer(disposalEnvironment, methodInfoOffset, disposalInfo);
    putPointer(disposalInfo, methodImplementationOffset, originalComplete);
    outerFrame = (AvmMethodFrame){NULL, disposalEnvironment, 0};
    putPointer(core, coreFrameOffset, &outerFrame);
    originalWriteInfo = writeInfoFile;
    originalRecoveryComplete = originalComplete;
    originalWaitAddBehavior = defaultAddBehavior;
    readProperty = readAddTail;
    recoveryWriteReturnAddress = 0x10534b98c;
    isActive = 1;
}
// //// /构造恢复生命周期的方法环境 ////

// //// 限定恢复进度对象的完成登记顺序 [@x380kkm 2026-09-27] ////
static void checkRecoveryCallbackOrder(void) {
    putPointer(behaviorEnvironment, methodInfoOffset, behaviorInfo);
    putPointer(behaviorEnvironment, methodScopeOffset, scope);
    putPointer(behaviorInfo, methodPoolOffset, pool);
    putPointer(pool, poolNamesOffset, names);
    putPointer(scope, scopeAbcEnvironmentOffset, abc);
    putPointer(abc, abcScriptEnvironmentsOffset, scripts);
    putPointer(scripts, behaviorScriptSlot, behaviorScript);
    putPointer(behaviorScript, scriptGlobalOffset, behaviorClass);
    for (size_t index = 0; index < 3; index += 1) {
        recoveryConstructionMethods[index] = 0x3000 + index * 0x1000;
        putPointer(constructionEnvironment[index], methodInfoOffset, constructionInfo[index]);
        putPointer(constructionInfo[index], methodImplementationOffset, (void *)recoveryConstructionMethods[index]);
        constructionFrames[index] = (AvmMethodFrame){
            index == 2 ? &outerFrame : &constructionFrames[index + 1], constructionEnvironment[index], 0
        };
    }
    assert(getRecoveryCompletionAddBehavior(&downloader, behaviorEnvironment) == (void *)0x1000);
    putPointer(core, coreFrameOffset, &constructionFrames[0]);
    assert(getRecoveryCompletionAddBehavior(&downloader, behaviorEnvironment) == (void *)0x2000);
    assert(behaviorReads == 1 && readPointer(core, coreFrameOffset) == &constructionFrames[0]);
    constructionFrames[1].environment = disposalEnvironment;
    assert(getRecoveryCompletionAddBehavior(&downloader, behaviorEnvironment) == (void *)0x1000);
    assert(behaviorReads == 1);
    putPointer(core, coreFrameOffset, &outerFrame);
}

// //// 验证完成提交和取消后的记录保留 [@x380kkm 2026-09-27] ////
int main(void) {
    prepare();
    checkRecoveryCallbackOrder();
    assert(writeRecoveryInfoIfComplete(&downloader, &info, writeEnvironment, 1) == 20);
    assert(writes == 1 && completions == 0);

    disposeFrame = (AvmMethodFrame){&outerFrame, disposalEnvironment, 0};
    putPointer(core, coreFrameOffset, &disposeFrame);
    assert(writeRecoveryInfo() == undefinedAtom);
    assert(writes == 1);
    putPointer(core, coreFrameOffset, NULL);
    assert(writeRecoveryInfo() == undefinedAtom);
    putPointer(core, coreFrameOffset, &outerFrame);

    assert(completeAssetRecovery(receiver, completeEnvironment) == 12);
    assert(writes == 2 && completions == 1);
    assert(readPointer(core, coreFrameOffset) == &outerFrame);
    assert(completeAssetRecovery(receiver, completeEnvironment) == undefinedAtom);
    assert(writes == 2 && completions == 1);

    isActive = 1;
    failWrite = true;
    if (setjmp(writeException) == 0) {
        completeAssetRecovery(receiver, completeEnvironment);
        assert(0);
    }
    failWrite = false;
    disposeFrame = (AvmMethodFrame){&outerFrame, disposalEnvironment, 0};
    putPointer(core, coreFrameOffset, &disposeFrame);
    assert(writeRecoveryInfo() == undefinedAtom);
    assert(writes == 2 && completions == 2);
    putPointer(core, coreFrameOffset, &outerFrame);
    assert(completeAssetRecovery(receiver, completeEnvironment) == undefinedAtom);
    assert(writes == 2 && completions == 2);
    puts("恢复回调按登记顺序执行并提交一次, 失败, 退出和迟到回调保留恢复记录.");
    return 0;
}
// //// /验证完成提交和取消后的记录保留 ////
