// audience: internal
// # cn-asset-file-tests
// 文件适配按资源调用位置检查内容和清单长度, 原生恢复删除操作保留真实的文件存在状态.

#include <assert.h>
#include <stdio.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNAssetFileHook.m"

_Alignas(void *) static uint8_t environment[0x20], info[0x58], pool[0x10], core[0x68];
_Alignas(void *) static uint8_t scope[0x18], abc[0x38], scripts[fileScriptSlot + sizeof(void *)];
_Alignas(void *) static uint8_t fileScript[0x28], fileClass[0x18], classVtable[0x28];
_Alignas(void *) static uint8_t file[0x18], fileVtable[fileDirectoryMethodSlot + sizeof(void *)];
_Alignas(void *) static uint8_t sizeEnvironment[0x18], sizeInfo[0x58], directoryEnvironment[0x18], directoryInfo[0x58];
_Alignas(void *) static uint8_t numberScript[0x18], numberVtable[parseFloatMethodSlot + sizeof(void *)];
_Alignas(void *) static uint8_t parserEnvironment[0x18], parserInfo[0x58], record, numberText;
static AvmMethodFrame callerFrame;
static uint32_t exists, directory;
static double size, expectedSize;
static size_t sizeReads, manifestReads, scriptLoads, interrupts;

// //// 提供文件属性与清单长度的原生接口 [@x380kkm 2026-09-27] ////
static void putPointer(void *object, size_t offset, const void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

static uint32_t fileExists(void *receiver, void *method) {
    assert(receiver == file && method == environment);
    return exists;
}

static uint32_t fileIsDirectory(void *receiver, void *method) {
    assert(receiver == file && method == directoryEnvironment);
    return directory;
}

static double fileSize(void *receiver, void *method) {
    assert(receiver == file && method == sizeEnvironment);
    sizeReads += 1;
    return size;
}

static uintptr_t manifestColumn(AvmMethodFrame *frame, uint32_t index, void *row) {
    assert(frame == &callerFrame && index == 2 && row == &record);
    manifestReads += 1;
    return 0xdec02;
}

static void *numberString(AvmMethodFrame *frame, uintptr_t value) {
    assert(frame == &callerFrame && value == 0xdec02);
    return &numberText;
}

static double parseNumber(void *receiver, void *text, void *method) {
    assert(receiver == numberScript && text == &numberText && method == parserEnvironment);
    return expectedSize;
}

static void *loadAssetScript(void **slot, AvmMethodFrame *frame) {
    assert(readPointer(core, coreFrameOffset) == frame && frame->environment == environment);
    scriptLoads += 1;
    *slot = slot == (void **)(scripts + fileScriptSlot) ? fileScript : numberScript;
    return *slot;
}

static void checkAssetInterrupt(void *receiver) {
    assert(receiver == core);
    interrupts += 1;
}
// //// /提供文件属性与清单长度的原生接口 ////

// //// 连接文件虚表和 AIR 调用帧 [@x380kkm 2026-09-27] ////
static void prepare(void) {
    callerFrame = (AvmMethodFrame){NULL, environment, 0};
    putPointer(environment, methodInfoOffset, info);
    putPointer(environment, methodScopeOffset, scope);
    putPointer(info, methodPoolOffset, pool);
    putPointer(info, methodImplementationOffset, fileExists);
    putPointer(pool, poolCoreOffset, core);
    putPointer(core, coreFrameOffset, &callerFrame);
    putPointer(scope, scopeAbcEnvironmentOffset, abc);
    putPointer(abc, abcScriptEnvironmentsOffset, scripts);
    putPointer(fileScript, scriptGlobalOffset, fileClass);
    putPointer(fileClass, objectVtableOffset, classVtable);
    putPointer(classVtable, vtableInstanceOffset, fileVtable);
    putPointer(file, objectVtableOffset, fileVtable);
    putPointer(fileVtable, fileExistsMethodSlot, environment);
    putPointer(fileVtable, fileDirectoryMethodSlot, directoryEnvironment);
    putPointer(directoryEnvironment, methodInfoOffset, directoryInfo);
    putPointer(directoryInfo, methodImplementationOffset, fileIsDirectory);
    putPointer(fileVtable, fileSizeMethodSlot, sizeEnvironment);
    putPointer(sizeEnvironment, methodInfoOffset, sizeInfo);
    putPointer(sizeInfo, methodImplementationOffset, fileSize);
    putPointer(numberScript, objectVtableOffset, numberVtable);
    putPointer(numberVtable, parseFloatMethodSlot, parserEnvironment);
    putPointer(parserEnvironment, methodInfoOffset, parserInfo);
    putPointer(parserInfo, methodImplementationOffset, parseNumber);
    originalFileExists = fileExists;
    readArrayElement = manifestColumn;
    convertString = numberString;
    initializeScript = loadAssetScript;
    checkInterrupt = checkAssetInterrupt;
    readerReturnAddress = 10;
    fullCheckReturnAddress = 20;
    shortenedCheckReturnAddress = 30;
    intermediateReturnAddress = 40;
    exists = 1;
    directory = 0;
    size = 126;
    expectedSize = 126;
}
// //// /连接文件虚表和 AIR 调用帧 ////

// //// 校验资源读取, 长度不符, 目录及恢复删除的边界 [@x380kkm 2026-09-27] ////
int main(void) {
    prepare();
    const uintptr_t resourceCalls[] = {10, 20, 30, 40};
    for (size_t index = 0; index < sizeof(resourceCalls) / sizeof(resourceCalls[0]); index += 1) {
        size = 0;
        assert(resolveAssetFileExists(file, environment, &record, resourceCalls[index]) == 0);
        size = 126;
        assert(resolveAssetFileExists(file, environment, &record, resourceCalls[index]) == 1);
    }
    expectedSize = 127;
    assert(resolveAssetFileExists(file, environment, &record, fullCheckReturnAddress) == 0);
    assert(resolveAssetFileExists(file, environment, &record, shortenedCheckReturnAddress) == 1);
    expectedSize = NAN;
    assert(resolveAssetFileExists(file, environment, &record, fullCheckReturnAddress) == 0);
    assert(resolveAssetFileExists(file, environment, NULL, fullCheckReturnAddress) == 0);
    size = NAN;
    assert(resolveAssetFileExists(file, environment, &record, readerReturnAddress) == 0);
    size = -1;
    assert(resolveAssetFileExists(file, environment, &record, readerReturnAddress) == 0);
    size = 0;
    size_t reads = sizeReads;
    assert(resolveAssetFileExists(file, environment, NULL, 100) == 1);
    assert(sizeReads == reads);
    exists = 0;
    assert(resolveAssetFileExists(file, environment, &record, fullCheckReturnAddress) == 0);
    assert(sizeReads == reads);
    exists = 1;
    directory = 1;
    assert(resolveAssetFileExists(file, environment, &record, readerReturnAddress) == 0);
    assert(sizeReads == reads && manifestReads == 3);
    assert(readPointer(core, coreFrameOffset) == &callerFrame);
    originalFileExists = NULL;
    prepareAssetFilePredicate(environment, fileIsDirectory);
    assert(originalFileExists == fileExists);
    assert(methodImplementation(environment) == (void *)fileIsDirectory);
    assert(readPointer(core, coreFrameOffset) == &callerFrame);
    assert(scriptLoads == 2 && interrupts == 2);
    prepareAssetFilePredicate(environment, fileExists);
    assert(methodImplementation(environment) == (void *)fileIsDirectory);
    assert(scriptLoads == 2 && interrupts == 2);
    puts("资源文件按内容与清单长度判定, 普通文件操作和 AIR 调用帧保持原有状态.");
    return 0;
}
// //// /校验资源读取, 长度不符, 目录及恢复删除的边界 ////
