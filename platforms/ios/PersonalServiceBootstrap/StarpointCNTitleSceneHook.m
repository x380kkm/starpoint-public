// audience: internal
// # cn-title-scene-hook
//
// 该模块只识别 CN iOS 1.8.4 主程序 UUID 4C4C4408-5555-3144-A151-6203E95DEFE1.
// AOT 描述项、方法表和原函数指针必须全部匹配, 否则不修改主程序内存.
// viewChanged 返回后显示管理入口, disposeTitleScene 执行前移除管理入口.

#import <mach-o/dyld.h>
#import <mach-o/loader.h>

#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#import "StarpointPersonalServiceBootstrap.h"

static const uint8_t StarpointCn184ExecutableUuid[16] = {
    0x4c, 0x4c, 0x44, 0x08, 0x55, 0x55, 0x31, 0x44,
    0xa1, 0x51, 0x62, 0x03, 0xe9, 0x5d, 0xef, 0xe1,
};
static const uint8_t StarpointCn184AbcDigest[16] = {
    0xa6, 0x39, 0x6d, 0xfd, 0x3f, 0xee, 0x07, 0x08,
    0x3d, 0x4e, 0x13, 0x9c, 0xaa, 0xc5, 0x21, 0x67,
};

static const uint64_t StarpointCn184AotDescriptorVm = 0x1063b4b80;
static const uint64_t StarpointCn184AbcVm = 0x1059aed10;
static const uint64_t StarpointCn184AbcLength = 0x56daa5;
static const uint64_t StarpointCn184MethodTableVm = 0x1062c0780;
static const uint64_t StarpointCn184MethodCount = 0x18acf;
static const uint64_t StarpointCn184ViewChangedVm = 0x1037d4538;
static const uint64_t StarpointCn184RunVm = 0x1037d2cec;
static const uint64_t StarpointCn184DisposeTitleSceneVm = 0x1037d08fc;
static const uint64_t StarpointCn184AfterTransitionVm = 0x1037cfb30;

static const uint64_t StarpointCn184ViewChangedMethodIndex = 91102;
static const uint64_t StarpointCn184RunMethodIndex = 91110;
static const uint64_t StarpointCn184DisposeTitleSceneMethodIndex = 91139;
static const uint64_t StarpointCn184AfterTransitionMethodIndex = 91141;

typedef uint64_t (*StarpointAotInstanceMethod)(void *receiver, void *methodEnvironment);

static StarpointAotInstanceMethod originalViewChanged;
static StarpointAotInstanceMethod originalDisposeTitleScene;

// //// 判断主程序是否为已确认的 CN iOS 1.8.4 [@x380kkm 2026-08-20] ////
static bool mainExecutableHasExpectedUuid(const struct mach_header_64 *header) {
    if (header == NULL || header->magic != MH_MAGIC_64 || header->cputype != CPU_TYPE_ARM64) {
        return false;
    }

    const uint8_t *cursor = (const uint8_t *)(header + 1);
    const uint8_t *commandsEnd = cursor + header->sizeofcmds;
    for (uint32_t index = 0; index < header->ncmds; index += 1) {
        if ((size_t)(commandsEnd - cursor) < sizeof(struct load_command)) {
            return false;
        }
        const struct load_command *command = (const struct load_command *)cursor;
        if (command->cmdsize < sizeof(struct load_command) ||
            (size_t)(commandsEnd - cursor) < command->cmdsize) {
            return false;
        }
        if (command->cmd == LC_UUID) {
            if (command->cmdsize < sizeof(struct uuid_command)) {
                return false;
            }
            const struct uuid_command *uuidCommand = (const struct uuid_command *)command;
            return memcmp(
                       uuidCommand->uuid,
                       StarpointCn184ExecutableUuid,
                       sizeof(StarpointCn184ExecutableUuid)
                   ) == 0;
        }
        cursor += command->cmdsize;
    }
    return false;
}
// //// /判断主程序是否为已确认的 CN iOS 1.8.4 ////

static uintptr_t runtimeAddress(uint64_t preferredAddress, intptr_t slide) {
    return (uintptr_t)((intptr_t)preferredAddress + slide);
}

// //// 验证 AOT 描述项和 TitleScene 方法表 [@x380kkm 2026-08-20] ////
static StarpointAotInstanceMethod *validatedTitleSceneMethodTable(intptr_t slide) {
    const uint8_t *descriptor =
        (const uint8_t *)runtimeAddress(StarpointCn184AotDescriptorVm, slide);
    if (memcmp(descriptor, StarpointCn184AbcDigest, sizeof(StarpointCn184AbcDigest)) != 0) {
        return NULL;
    }

    uintptr_t abcAddress = *(const uintptr_t *)(descriptor + 0x18);
    uint64_t abcLength = *(const uint64_t *)(descriptor + 0x20);
    uintptr_t methodTableAddress = *(const uintptr_t *)(descriptor + 0x30);
    uint64_t methodCount = *(const uint64_t *)(descriptor + 0x38);
    if (abcAddress != runtimeAddress(StarpointCn184AbcVm, slide) ||
        abcLength != StarpointCn184AbcLength ||
        methodTableAddress != runtimeAddress(StarpointCn184MethodTableVm, slide) ||
        methodCount != StarpointCn184MethodCount ||
        methodCount <= StarpointCn184AfterTransitionMethodIndex) {
        return NULL;
    }

    StarpointAotInstanceMethod *methodTable =
        (StarpointAotInstanceMethod *)methodTableAddress;
    if ((uintptr_t)methodTable[StarpointCn184ViewChangedMethodIndex] !=
            runtimeAddress(StarpointCn184ViewChangedVm, slide) ||
        (uintptr_t)methodTable[StarpointCn184RunMethodIndex] !=
            runtimeAddress(StarpointCn184RunVm, slide) ||
        (uintptr_t)methodTable[StarpointCn184DisposeTitleSceneMethodIndex] !=
            runtimeAddress(StarpointCn184DisposeTitleSceneVm, slide) ||
        (uintptr_t)methodTable[StarpointCn184AfterTransitionMethodIndex] !=
            runtimeAddress(StarpointCn184AfterTransitionVm, slide)) {
        return NULL;
    }
    return methodTable;
}
// //// /验证 AOT 描述项和 TitleScene 方法表 ////

// //// 在标题视图完成切换后显示管理入口 [@x380kkm 2026-08-20] ////
static uint64_t titleSceneViewChanged(void *receiver, void *methodEnvironment) {
    uint64_t result = originalViewChanged(receiver, methodEnvironment);
    starpoint_personal_service_bootstrap_set_management_entry_visible(1);
    return result;
}
// //// /在标题视图完成切换后显示管理入口 ////

// //// 在标题场景释放前移除管理入口 [@x380kkm 2026-08-20] ////
static uint64_t titleSceneDispose(void *receiver, void *methodEnvironment) {
    starpoint_personal_service_bootstrap_set_management_entry_visible(0);
    return originalDisposeTitleScene(receiver, methodEnvironment);
}
// //// /在标题场景释放前移除管理入口 ////

// //// 安装精确匹配的 TitleScene AOT 方法表钩子 [@x380kkm 2026-08-20] ////
__attribute__((constructor)) static void installCnTitleSceneHook(void) {
    const struct mach_header *imageHeader = _dyld_get_image_header(0);
    const struct mach_header_64 *header = (const struct mach_header_64 *)imageHeader;
    if (!mainExecutableHasExpectedUuid(header)) {
        return;
    }

    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    StarpointAotInstanceMethod *methodTable = validatedTitleSceneMethodTable(slide);
    if (methodTable == NULL) {
        return;
    }

    originalViewChanged = methodTable[StarpointCn184ViewChangedMethodIndex];
    originalDisposeTitleScene = methodTable[StarpointCn184DisposeTitleSceneMethodIndex];
    methodTable[StarpointCn184ViewChangedMethodIndex] = titleSceneViewChanged;
    methodTable[StarpointCn184DisposeTitleSceneMethodIndex] = titleSceneDispose;
    atomic_thread_fence(memory_order_release);
}
// //// /安装精确匹配的 TitleScene AOT 方法表钩子 ////
