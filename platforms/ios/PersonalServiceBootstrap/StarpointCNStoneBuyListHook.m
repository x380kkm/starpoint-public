// audience: internal
// # cn-stone-buy-list-hook
// 商品列表重绘前按当前横幅行同步额外按钮, 已有按钮保留点击回调和可用状态.
// 对象布局与方法槽位对应 CN iOS 1.8.4, 按钮增删沿用 AIR 的 ButtonGroupLogic 生命周期.

#include <mach-o/dyld.h>
#include <stdbool.h>
#include <stdatomic.h>

#include "StarpointCNAotRuntime.h"

enum {
    refreshListMethodIndex = 40476,
    objectVtableOffset = 0x10,
    methodInfoOffset = 0x10,
    methodImplementationOffset = 0x50,
    methodPoolOffset = 0x30,
    poolCoreOffset = 0x08,
    coreFrameOffset = 0x60,
    viewPeekOffset = 0x130,
    dialogItemListOffset = 0x1a8,
    listButtonGroupOffset = 0x120,
    enumIndexOffset = 0x20,
    listDataSourceMethodSlot = 0x150,
    productDisplayKindMethodSlot = 0x1a0,
    arrayLengthMethodSlot = 0xb8,
    buttonKindsMethodSlot = 0x138,
    buttonRemoveMethodSlot = 0x108,
    buttonExistsMethodSlot = 0x168,
    buttonAppendMethodSlot = 0x1a8,
};

typedef struct {
    void *previous;
    void *environment;
    uintptr_t defaultXmlNamespace;
} AvmMethodFrame;

typedef uintptr_t (*VoidMethod)(void *, void *);
typedef void *(*ObjectGetter)(void *, void *);
typedef uint32_t (*ArrayLengthGetter)(void *, void *);
typedef uintptr_t (*ArrayElementGetter)(AvmMethodFrame *, uint32_t, void *);
typedef int32_t (*IntegerConverter)(AvmMethodFrame *, uintptr_t);
typedef uintptr_t (*IndexedVoidMethod)(void *, int32_t, void *);
typedef uint32_t (*IndexedPredicate)(void *, int32_t, void *);
typedef void *(*IndexedObjectGetter)(void *, int32_t, void *);
typedef void (*InterruptCheck)(void *);
typedef void (*NullObjectError)(void *);

static VoidMethod originalRefreshList;
static ArrayElementGetter getArrayElement;
static IntegerConverter toInteger;
static InterruptCheck checkInterrupt;
static NullObjectError raiseNullObjectError;

// //// 通过对象虚表读取 AIR 方法与数组 [@x380kkm 2026-09-28] ////
static void *readPointer(void *object, size_t offset) {
    return *(void **)((uint8_t *)object + offset);
}

static void *requirePointer(void *object, void *environment) {
    if (object == NULL) {
        raiseNullObjectError(environment);
        __builtin_unreachable();
    }
    return object;
}

static void *methodEnvironment(void *receiver, size_t slot) {
    return readPointer(readPointer(receiver, objectVtableOffset), slot);
}

static void *methodImplementation(void *environment) {
    return readPointer(readPointer(environment, methodInfoOffset), methodImplementationOffset);
}

static void *getObject(void *receiver, size_t slot, void *environment) {
    void *method = methodEnvironment(receiver, slot);
    return requirePointer(((ObjectGetter)methodImplementation(method))(receiver, method), environment);
}

static uint32_t arrayLength(void *array) {
    void *method = methodEnvironment(array, arrayLengthMethodSlot);
    return ((ArrayLengthGetter)methodImplementation(method))(array, method);
}

static bool isBanner(void *products, uint32_t index, AvmMethodFrame *frame) {
    uintptr_t value = getArrayElement(frame, index, products);
    void *product = requirePointer((void *)(value & ~(uintptr_t)7), frame->environment);
    void *displayKind = getObject(product, productDisplayKindMethodSlot, frame->environment);
    return *(uint32_t *)((uint8_t *)displayKind + enumIndexOffset) == 1;
}
// //// /通过对象虚表读取 AIR 方法与数组 ////

// //// 删除离开列表的横幅按钮并注册当前横幅按钮 [@x380kkm 2026-09-28] ////
static void synchronizeBannerButtons(void *list, AvmMethodFrame *frame) {
    void *group = requirePointer(readPointer(list, listButtonGroupOffset), frame->environment);
    void *products = getObject(list, listDataSourceMethodSlot, frame->environment);
    void *kinds = getObject(group, buttonKindsMethodSlot, frame->environment);
    uint32_t productCount = arrayLength(products);
    void *removeMethod = methodEnvironment(group, buttonRemoveMethodSlot);
    for (uint32_t remaining = arrayLength(kinds); remaining > 0; remaining -= 1) {
        int32_t index = toInteger(frame, getArrayElement(frame, remaining - 1, kinds));
        if (index < 0 || (uint32_t)index >= productCount || !isBanner(products, (uint32_t)index, frame)) {
            ((IndexedVoidMethod)methodImplementation(removeMethod))(group, index, removeMethod);
        }
    }
    void *existsMethod = methodEnvironment(group, buttonExistsMethodSlot);
    void *appendMethod = methodEnvironment(group, buttonAppendMethodSlot);
    for (uint32_t index = 0; index < productCount; index += 1) {
        if (isBanner(products, index, frame) &&
            !((IndexedPredicate)methodImplementation(existsMethod))(group, (int32_t)index, existsMethod)) {
            ((IndexedObjectGetter)methodImplementation(appendMethod))(group, (int32_t)index, appendMethod);
        }
    }
}
// //// /删除离开列表的横幅按钮并注册当前横幅按钮 ////

// //// 在当前页签单元格绘制前同步额外按钮 [@x380kkm 2026-09-28] ////
static uintptr_t refreshStoneBuyList(void *receiver, void *environment) {
    void *pool = readPointer(readPointer(environment, methodInfoOffset), methodPoolOffset);
    void *core = readPointer(pool, poolCoreOffset);
    void **frameSlot = (void **)((uint8_t *)core + coreFrameOffset);
    checkInterrupt(core);
    AvmMethodFrame frame = {*frameSlot, environment, 0};
    *frameSlot = &frame;

    void *dialog = requirePointer(readPointer(receiver, viewPeekOffset), environment);
    void *list = requirePointer(readPointer(dialog, dialogItemListOffset), environment);
    synchronizeBannerButtons(list, &frame);
    uintptr_t result = originalRefreshList(receiver, environment);
    checkInterrupt(core);
    *frameSlot = frame.previous;
    return result;
}
// //// /在当前页签单元格绘制前同步额外按钮 ////

// //// 连接版本匹配的商店列表刷新方法 [@x380kkm 2026-09-28] ////
__attribute__((constructor)) static void installCnStoneBuyListHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {refreshListMethodIndex, 0x1052d6ab0},
        {40443, 0x1052d36f0},
        {40478, 0x1052d5794},
        {95958, 0x1039d6470},
        {39862, 0x103a07ef8},
        {22273, 0x104a6428c},
        {76713, 0x103aa77dc},
        {76719, 0x103aa62c8},
        {76725, 0x103aa5620},
        {76733, 0x103aa4548},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL) {
        return;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    originalRefreshList = (VoidMethod)table[refreshListMethodIndex];
    getArrayElement = (ArrayElementGetter)(0x100a27090 + slide);
    toInteger = (IntegerConverter)(0x100a26a20 + slide);
    checkInterrupt = (InterruptCheck)(0x100a26814 + slide);
    raiseNullObjectError = (NullObjectError)(0x100a2676c + slide);
    table[refreshListMethodIndex] = (uintptr_t)refreshStoneBuyList;
    atomic_thread_fence(memory_order_release);
}
// //// /连接版本匹配的商店列表刷新方法 ////
