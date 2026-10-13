// audience: internal
// # cn-asset-file-hook
// 资源读取, 完整性检查和中间下载仅接受有内容的文件. 完整资源同时匹配实体清单长度.
// 调用地址, 对象布局和 AIR 方法槽位对应 CN iOS 1.8.4. 资源恢复继续使用客户端的异步下载与缓存刷新.

#include <mach-o/dyld.h>
#include <math.h>
#include <stdatomic.h>

#include "StarpointCNAotRuntime.h"

enum {
    fileReaderExistsIndex = 5630,
    intermediateStartIndex = 5675,
    sufficiencyUpdateIndex = 41644,
    objectVtableOffset = 0x10,
    vtableInstanceOffset = 0x20,
    methodInfoOffset = 0x10,
    methodImplementationOffset = 0x50,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    coreFrameOffset = 0x60,
    methodScopeOffset = 0x18,
    scopeAbcEnvironmentOffset = 0x10,
    abcScriptEnvironmentsOffset = 0x30,
    scriptGlobalOffset = 0x20,
    fileScriptSlot = 0x6cb8,
    numberScriptSlot = 0x8b8,
    parseFloatMethodSlot = 0xd8,
    fileSizeMethodSlot = 0x120,
    fileExistsMethodSlot = 0x1b8,
    fileDirectoryMethodSlot = 0x1e8,
};

typedef struct {
    void *previous;
    void *environment;
    uintptr_t defaultXmlNamespace;
} AvmMethodFrame;

typedef uint32_t (*FilePredicate)(void *, void *);
typedef double (*NumberGetter)(void *, void *);
typedef double (*StringNumberParser)(void *, void *, void *);
typedef uint32_t (*FileReaderPredicate)(void *, void *, void *);
typedef uintptr_t (*UpdateMethod)(void *, void *);
typedef void *(*ScriptInitializer)(void **, AvmMethodFrame *);
typedef void (*InterruptCheck)(void *);
typedef uintptr_t (*ArrayElementReader)(AvmMethodFrame *, uint32_t, void *);
typedef void *(*StringConverter)(AvmMethodFrame *, uintptr_t);

static _Atomic(FilePredicate) originalFileExists;
#if defined(__arm64__)
static FileReaderPredicate originalReaderExists;
static UpdateMethod originalSufficiencyUpdate;
static UpdateMethod originalIntermediateStart;
#endif
static ScriptInitializer initializeScript;
static InterruptCheck checkInterrupt;
static ArrayElementReader readArrayElement;
static StringConverter convertString;
static uintptr_t readerReturnAddress;
static uintptr_t fullCheckReturnAddress;
static uintptr_t shortenedCheckReturnAddress;
static uintptr_t intermediateReturnAddress;

// //// 读取 AIR 对象的方法环境与实现 [@x380kkm 2026-09-27] ////
static void *readPointer(const void *object, size_t offset) {
    return *(void *const *)((const uint8_t *)object + offset);
}

static void *methodEnvironment(void *object, size_t slot) {
    return readPointer(readPointer(object, objectVtableOffset), slot);
}

static void *methodImplementation(void *environment) {
    return readPointer(readPointer(environment, methodInfoOffset), methodImplementationOffset);
}

static void *methodCore(void *environment) {
    return readPointer(readPointer(readPointer(environment, methodInfoOffset), methodPoolOffset), poolCoreOffset);
}

static void *loadScript(void *environment, size_t offset, AvmMethodFrame *frame) {
    void *scope = readPointer(environment, methodScopeOffset);
    void *abc = readPointer(scope, scopeAbcEnvironmentOffset);
    void *scripts = readPointer(abc, abcScriptEnvironmentsOffset);
    void **slot = (void **)((uint8_t *)scripts + offset);
    return *slot == NULL ? initializeScript(slot, frame) : *slot;
}
// //// /读取 AIR 对象的方法环境与实现 ////

// //// 使用清单行的长度字段检查完整文件 [@x380kkm 2026-09-27] ////
static double manifestFileSize(void *record, void *environment) {
    AvmMethodFrame *frame = readPointer(methodCore(environment), coreFrameOffset);
    if (frame == NULL || record == NULL) {
        return NAN;
    }
    uintptr_t value = readArrayElement(frame, 2, record);
    void *text = convertString(frame, value);
    void *script = loadScript(frame->environment, numberScriptSlot, frame);
    void *parser = methodEnvironment(script, parseFloatMethodSlot);
    return ((StringNumberParser)methodImplementation(parser))(script, text, parser);
}

__attribute__((used, noinline)) static uint32_t resolveAssetFileExists(
    void *file, void *environment, void *record, uintptr_t returnAddress
) {
    FilePredicate readExists = atomic_load_explicit(&originalFileExists, memory_order_acquire);
    uint32_t exists = readExists(file, environment);
    if (!exists || (returnAddress != readerReturnAddress && returnAddress != fullCheckReturnAddress &&
                    returnAddress != shortenedCheckReturnAddress && returnAddress != intermediateReturnAddress)) {
        return exists;
    }
    void *directory = methodEnvironment(file, fileDirectoryMethodSlot);
    if (((FilePredicate)methodImplementation(directory))(file, directory)) {
        return 0;
    }
    void *size = methodEnvironment(file, fileSizeMethodSlot);
    double actual = ((NumberGetter)methodImplementation(size))(file, size);
    if (!isfinite(actual) || actual <= 0) {
        return 0;
    }
    return returnAddress == fullCheckReturnAddress ? actual == manifestFileSize(record, environment) : 1;
}
// //// /使用清单行的长度字段检查完整文件 ////

#if defined(__arm64__)
// //// 将完整性检查的清单行与资源调用位置传入文件判断 [@x380kkm 2026-09-27] ////
__attribute__((naked)) static uint32_t assetFileExists(
    void *file __attribute__((unused)), void *environment __attribute__((unused))
) {
    __asm__ volatile(
        "mov x2, x27\n"
        "mov x3, x30\n"
        "b _resolveAssetFileExists\n"
    );
}
#endif

// //// 在资源入口初始化文件类型并连接长度判断 [@x380kkm 2026-09-27] ////
__attribute__((used)) static void prepareAssetFilePredicate(void *environment, FilePredicate predicate) {
    if (atomic_load_explicit(&originalFileExists, memory_order_acquire) != NULL) {
        return;
    }
    void *core = methodCore(environment);
    void **frameSlot = (void **)((uint8_t *)core + coreFrameOffset);
    checkInterrupt(core);
    AvmMethodFrame frame = {*frameSlot, environment, 0};
    *frameSlot = &frame;
    void *script = loadScript(environment, fileScriptSlot, &frame);
    void *fileClass = readPointer(script, scriptGlobalOffset);
    void *instanceVtable = readPointer(readPointer(fileClass, objectVtableOffset), vtableInstanceOffset);
    void *getter = readPointer(instanceVtable, fileExistsMethodSlot);
    void *info = readPointer(getter, methodInfoOffset);
    _Atomic(uintptr_t) *implementation = (_Atomic(uintptr_t) *)((uint8_t *)info + methodImplementationOffset);
    FilePredicate current = (FilePredicate)atomic_load_explicit(implementation, memory_order_acquire);
    FilePredicate absent = NULL;
    if (atomic_compare_exchange_strong_explicit(&originalFileExists, &absent, current,
                                               memory_order_acq_rel, memory_order_acquire)) {
        atomic_store_explicit(implementation, (uintptr_t)predicate, memory_order_release);
    }
    checkInterrupt(core);
    *frameSlot = frame.previous;
}
// //// /在资源入口初始化文件类型并连接长度判断 ////

#if defined(__arm64__)
// //// 保留资源入口的参数, 返回值和异步流程 [@x380kkm 2026-09-27] ////
static uint32_t readerFileExists(void *receiver, void *path, void *environment) {
    prepareAssetFilePredicate(environment, assetFileExists);
    return originalReaderExists(receiver, path, environment);
}

static uintptr_t updateAssetSufficiency(void *receiver, void *environment) {
    prepareAssetFilePredicate(environment, assetFileExists);
    return originalSufficiencyUpdate(receiver, environment);
}

static uintptr_t startIntermediateDownload(void *receiver, void *environment) {
    prepareAssetFilePredicate(environment, assetFileExists);
    return originalIntermediateStart(receiver, environment);
}
// //// /保留资源入口的参数, 返回值和异步流程 ////

// //// 连接版本匹配的资源入口与原生文件调用点 [@x380kkm 2026-09-27] ////
__attribute__((constructor)) static void installCnAssetFileHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {fileReaderExistsIndex, 0x103eaf458},
        {sufficiencyUpdateIndex, 0x10534a524},
        {intermediateStartIndex, 0x103eb84c8},
        {5738, 0x103ec067c},
        {145, 0x10448cfb4},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    const uintptr_t calls[] = {0x103eaf678, 0x10534a9cc, 0x10534aa1c, 0x103eb87e4};
    for (size_t index = 0; index < sizeof(calls) / sizeof(calls[0]); index += 1) {
        if (*(const uint32_t *)(calls[index] + slide) != 0xd63f0100) {
            return;
        }
    }
    readerReturnAddress = calls[0] + slide + 4;
    fullCheckReturnAddress = calls[1] + slide + 4;
    shortenedCheckReturnAddress = calls[2] + slide + 4;
    intermediateReturnAddress = calls[3] + slide + 4;
    initializeScript = (ScriptInitializer)(0x10050a8e8 + slide);
    checkInterrupt = (InterruptCheck)(0x100a26814 + slide);
    readArrayElement = (ArrayElementReader)(0x100a27088 + slide);
    convertString = (StringConverter)(0x100a273ac + slide);
    originalReaderExists = (FileReaderPredicate)table[fileReaderExistsIndex];
    originalSufficiencyUpdate = (UpdateMethod)table[sufficiencyUpdateIndex];
    originalIntermediateStart = (UpdateMethod)table[intermediateStartIndex];
    table[fileReaderExistsIndex] = (uintptr_t)readerFileExists;
    table[sufficiencyUpdateIndex] = (uintptr_t)updateAssetSufficiency;
    table[intermediateStartIndex] = (uintptr_t)startIntermediateDownload;
    atomic_thread_fence(memory_order_release);
}
// //// /连接版本匹配的资源入口与原生文件调用点 ////
#endif
