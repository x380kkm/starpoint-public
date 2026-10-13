// audience: internal
// # cn-stone-buy-list-tests
// 此测试使用原生对象布局和按钮边界替身核对页签切换, 回调绑定及已有按钮状态.

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNStoneBuyListHook.m"

typedef struct {
    uint8_t header[0x10];
    void *vtable;
    uint32_t length;
    uintptr_t entries[8];
} TestArray;

typedef struct {
    uint8_t header[0x10];
    void *vtable;
    void *displayKind;
} TestProduct;

typedef struct {
    uint8_t environment[0x18];
    uint8_t info[0x58];
} TestMethod;

static uint8_t view[0x138], dialog[0x1b0], list[0x128], group[0x18];
static uint8_t viewEnvironment[0x18], viewInfo[0x58], pool[0x10], core[0x68];
static uint8_t listVtable[0x158], groupVtable[0x1b0], productVtable[0x1a8], arrayVtable[0xc0];
static uint8_t thumbnail[0x28], banner[0x28], previousFrame;
static TestMethod dataMethod, kindsMethod, lengthMethod, displayMethod, removeMethod, existsMethod, appendMethod;
static TestArray products, kinds;
static TestProduct thumbnailProduct, bannerProduct;
static bool enabled[8], registered[8], canValidate;
static void *callbackGroups[8];
static unsigned appended, removed, refreshed, interrupts;
static int32_t clickedIndex = -1;
static uintptr_t methods[95959] = {
    [40476] = 0x1052d6ab0, [40443] = 0x1052d36f0, [40478] = 0x1052d5794,
    [95958] = 0x1039d6470, [39862] = 0x103a07ef8, [22273] = 0x104a6428c,
    [76713] = 0x103aa77dc, [76719] = 0x103aa62c8, [76725] = 0x103aa5620,
    [76733] = 0x103aa4548,
};

// //// 建立可观察的客户端对象和方法环境 [@x380kkm 2026-09-28] ////
uintptr_t *starpointCnValidatedMethodTable(const StarpointCnMethodSignature *signatures, size_t count) {
    if (!canValidate) {
        return NULL;
    }
    for (size_t index = 0; index < count; index += 1) {
        assert(methods[signatures[index].index] == signatures[index].address);
    }
    return methods;
}

static void putPointer(void *object, size_t offset, void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

static void bindMethod(void *vtable, size_t slot, TestMethod *method, void *implementation) {
    putPointer(vtable, slot, method->environment);
    putPointer(method->environment, methodInfoOffset, method->info);
    putPointer(method->info, methodImplementationOffset, implementation);
}

static void *currentProducts(void *receiver, void *environment) {
    assert(receiver == list && environment == dataMethod.environment);
    return &products;
}

static void *currentKinds(void *receiver, void *environment) {
    assert(receiver == group && environment == kindsMethod.environment);
    return &kinds;
}

static uint32_t lengthOf(void *receiver, void *environment) {
    assert(environment == lengthMethod.environment);
    return ((TestArray *)receiver)->length;
}

static void *displayKind(void *receiver, void *environment) {
    assert(environment == displayMethod.environment);
    return ((TestProduct *)receiver)->displayKind;
}

static uintptr_t elementAt(AvmMethodFrame *frame, uint32_t index, void *array) {
    assert(frame == readPointer(core, coreFrameOffset));
    assert(frame->environment == viewEnvironment && frame->previous == &previousFrame);
    assert(index < ((TestArray *)array)->length);
    return ((TestArray *)array)->entries[index];
}

static int32_t integerValue(AvmMethodFrame *frame, uintptr_t value) {
    assert(frame == readPointer(core, coreFrameOffset));
    assert((value & 7) == 6);
    return (int32_t)(value >> 3);
}
// //// /建立可观察的客户端对象和方法环境 ////

// //// 模拟原按钮组的增删和绑定回调 [@x380kkm 2026-09-28] ////
static uint32_t buttonExists(void *receiver, int32_t index, void *environment) {
    assert(receiver == group && environment == existsMethod.environment);
    assert(index >= 0 && index < 8);
    return registered[index];
}

static void *appendButton(void *receiver, int32_t index, void *environment) {
    assert(receiver == group && environment == appendMethod.environment);
    assert(!registered[index]);
    kinds.entries[kinds.length++] = ((uintptr_t)index << 3) | 6;
    registered[index] = true;
    enabled[index] = true;
    callbackGroups[index] = receiver;
    appended += 1;
    return &registered[index];
}

static uintptr_t removeButton(void *receiver, int32_t index, void *environment) {
    assert(receiver == group && environment == removeMethod.environment);
    assert(registered[index]);
    uint32_t position = 0;
    while (position < kinds.length && (int32_t)(kinds.entries[position] >> 3) != index) {
        position += 1;
    }
    assert(position < kinds.length);
    memmove(kinds.entries + position, kinds.entries + position + 1,
            (kinds.length - position - 1) * sizeof(uintptr_t));
    kinds.length -= 1;
    registered[index] = false;
    callbackGroups[index] = NULL;
    removed += 1;
    return 4;
}

static uintptr_t renderList(void *receiver, void *environment) {
    assert(receiver == view && environment == viewEnvironment);
    for (uint32_t index = 0; index < products.length; index += 1) {
        TestProduct *product = (TestProduct *)(products.entries[index] & ~(uintptr_t)7);
        bool hasBanner = product->displayKind == banner;
        assert(registered[index] == hasBanner);
        if (hasBanner) {
            assert(callbackGroups[index] == group);
            if (enabled[index]) {
                clickedIndex = (int32_t)index;
            }
        }
    }
    refreshed += 1;
    return 4;
}

static void checkFrame(void *receiver) {
    assert(receiver == core);
    interrupts += 1;
}

static void unexpectedNull(void *environment) {
    (void)environment;
    abort();
}
// //// /模拟原按钮组的增删和绑定回调 ////

// //// 配置页签数据并执行真实同步入口 [@x380kkm 2026-09-28] ////
static void showProducts(uint32_t count, uint32_t bannerBits) {
    products.length = count;
    for (uint32_t index = 0; index < count; index += 1) {
        void *product = (bannerBits & (1u << index)) ? (void *)&bannerProduct : (void *)&thumbnailProduct;
        products.entries[index] = (uintptr_t)product | 1;
    }
    assert(refreshStoneBuyList(view, viewEnvironment) == 4);
    assert(readPointer(core, coreFrameOffset) == &previousFrame);
}

static void initializeObjects(void) {
    putPointer(view, viewPeekOffset, dialog);
    putPointer(dialog, dialogItemListOffset, list);
    putPointer(list, listButtonGroupOffset, group);
    putPointer(list, objectVtableOffset, listVtable);
    putPointer(group, objectVtableOffset, groupVtable);
    putPointer(viewEnvironment, methodInfoOffset, viewInfo);
    putPointer(viewInfo, methodPoolOffset, pool);
    putPointer(pool, poolCoreOffset, core);
    putPointer(core, coreFrameOffset, &previousFrame);
    *(uint32_t *)(banner + enumIndexOffset) = 1;
    thumbnailProduct.vtable = bannerProduct.vtable = productVtable;
    thumbnailProduct.displayKind = thumbnail;
    bannerProduct.displayKind = banner;
    products.vtable = kinds.vtable = arrayVtable;
    bindMethod(listVtable, listDataSourceMethodSlot, &dataMethod, currentProducts);
    bindMethod(groupVtable, buttonKindsMethodSlot, &kindsMethod, currentKinds);
    bindMethod(groupVtable, buttonRemoveMethodSlot, &removeMethod, removeButton);
    bindMethod(groupVtable, buttonExistsMethodSlot, &existsMethod, buttonExists);
    bindMethod(groupVtable, buttonAppendMethodSlot, &appendMethod, appendButton);
    bindMethod(productVtable, productDisplayKindMethodSlot, &displayMethod, displayKind);
    bindMethod(arrayVtable, arrayLengthMethodSlot, &lengthMethod, lengthOf);
    originalRefreshList = renderList;
    getArrayElement = elementAt;
    toInteger = integerValue;
    checkInterrupt = checkFrame;
    raiseNullObjectError = unexpectedNull;
}
// //// /配置页签数据并执行真实同步入口 ////

// //// 核对索引四横幅, 重复刷新, 反向切页及空列表 [@x380kkm 2026-09-28] ////
int main(void) {
    canValidate = true;
    installCnStoneBuyListHook();
    assert(methods[refreshListMethodIndex] == (uintptr_t)refreshStoneBuyList);
    initializeObjects();
    showProducts(2, 0);
    assert(kinds.length == 0 && appended == 0);
    showProducts(6, 1u << 4);
    assert(kinds.length == 1 && appended == 1 && clickedIndex == 4);
    enabled[4] = false;
    showProducts(6, 1u << 4);
    assert(!enabled[4] && appended == 1 && removed == 0);
    showProducts(7, 1u);
    assert(kinds.length == 1 && registered[0] && !registered[4] && clickedIndex == 0);
    showProducts(0, 0);
    assert(kinds.length == 0 && removed == 2);
    showProducts(6, (1u << 0) | (1u << 4));
    showProducts(2, 0);
    assert(kinds.length == 0 && appended == 4 && removed == 4);
    assert(refreshed == 7 && interrupts == 14);
    puts("商品横幅按钮随当前列表增删, 点击绑定和已有可用状态保留, AIR 调用帧恢复.");
    return 0;
}
// //// /核对索引四横幅, 重复刷新, 反向切页及空列表 ////
