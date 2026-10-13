// audience: internal
// # cn-aot-runtime
// 所有地址使用 CN iOS 1.8.4 的首选映像地址, 访问时加上 dyld 提供的偏移.

#import <mach-o/dyld.h>
#import <mach-o/loader.h>

#include <stdbool.h>
#include <string.h>

#import "StarpointCNAotRuntime.h"

static const uint8_t executableUuid[16] = {
    0x4c, 0x4c, 0x44, 0x08, 0x55, 0x55, 0x31, 0x44,
    0xa1, 0x51, 0x62, 0x03, 0xe9, 0x5d, 0xef, 0xe1,
};
static const uint8_t abcDigest[16] = {
    0xa6, 0x39, 0x6d, 0xfd, 0x3f, 0xee, 0x07, 0x08,
    0x3d, 0x4e, 0x13, 0x9c, 0xaa, 0xc5, 0x21, 0x67,
};
static const uintptr_t descriptorAddress = 0x1063b4b80;
static const uintptr_t abcAddress = 0x1059aed10;
static const uint64_t abcLength = 0x56daa5;
static const uintptr_t methodTableAddress = 0x1062c0780;
static const uint64_t methodCount = 0x18acf;

// //// 识别主程序的 Mach-O UUID [@x380kkm 2026-09-20] ////
static bool hasExpectedExecutableUuid(const struct mach_header_64 *header) {
    if (header == NULL || header->magic != MH_MAGIC_64 || header->cputype != CPU_TYPE_ARM64) {
        return false;
    }
    const uint8_t *cursor = (const uint8_t *)(header + 1);
    const uint8_t *end = cursor + header->sizeofcmds;
    for (uint32_t index = 0; index < header->ncmds; index += 1) {
        if ((size_t)(end - cursor) < sizeof(struct load_command)) {
            return false;
        }
        const struct load_command *command = (const struct load_command *)cursor;
        if (command->cmdsize < sizeof(struct load_command) ||
            (size_t)(end - cursor) < command->cmdsize) {
            return false;
        }
        if (command->cmd == LC_UUID) {
            if (command->cmdsize < sizeof(struct uuid_command)) {
                return false;
            }
            const struct uuid_command *uuid = (const struct uuid_command *)command;
            return memcmp(uuid->uuid, executableUuid, sizeof(executableUuid)) == 0;
        }
        cursor += command->cmdsize;
    }
    return false;
}
// //// /识别主程序的 Mach-O UUID ////

// //// 按原函数签名验证 AOT 方法表 [@x380kkm 2026-09-20] ////
uintptr_t *starpointCnValidatedMethodTable(
    const StarpointCnMethodSignature *signatures,
    size_t count
) {
    const struct mach_header_64 *header = (const struct mach_header_64 *)_dyld_get_image_header(0);
    if (!hasExpectedExecutableUuid(header)) {
        return NULL;
    }
    intptr_t slide = _dyld_get_image_vmaddr_slide(0);
    const uint8_t *descriptor = (const uint8_t *)(descriptorAddress + slide);
    if (memcmp(descriptor, abcDigest, sizeof(abcDigest)) != 0 ||
        *(const uintptr_t *)(descriptor + 0x18) != abcAddress + slide ||
        *(const uint64_t *)(descriptor + 0x20) != abcLength ||
        *(const uintptr_t *)(descriptor + 0x30) != methodTableAddress + slide ||
        *(const uint64_t *)(descriptor + 0x38) != methodCount) {
        return NULL;
    }
    uintptr_t *table = (uintptr_t *)(methodTableAddress + slide);
    for (size_t index = 0; index < count; index += 1) {
        if (signatures[index].index >= methodCount ||
            table[signatures[index].index] != signatures[index].address + slide) {
            return NULL;
        }
    }
    return table;
}
// //// /按原函数签名验证 AOT 方法表 ////
