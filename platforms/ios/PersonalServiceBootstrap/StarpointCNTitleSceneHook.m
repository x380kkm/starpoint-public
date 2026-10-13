// audience: internal
// # cn-title-scene-hook
//
// viewChanged 返回后显示管理入口, disposeTitleScene 执行前移除管理入口.

#include <stdatomic.h>

#import "StarpointCNAotRuntime.h"
#import "StarpointPersonalServiceBootstrap.h"

static const size_t viewChangedMethodIndex = 91102;
static const size_t disposeTitleSceneMethodIndex = 91139;

typedef uint64_t (*StarpointAotInstanceMethod)(void *receiver, void *methodEnvironment);

static StarpointAotInstanceMethod originalViewChanged;
static StarpointAotInstanceMethod originalDisposeTitleScene;

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

// //// 安装精确匹配的 TitleScene AOT 方法表钩子 [@x380kkm 2026-09-20] ////
__attribute__((constructor)) static void installCnTitleSceneHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {viewChangedMethodIndex, 0x1037d4538},
        {91110, 0x1037d2cec},
        {disposeTitleSceneMethodIndex, 0x1037d08fc},
        {91141, 0x1037cfb30},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(
        signatures, sizeof(signatures) / sizeof(signatures[0])
    );
    if (table == NULL) {
        return;
    }
    originalViewChanged = (StarpointAotInstanceMethod)table[viewChangedMethodIndex];
    originalDisposeTitleScene = (StarpointAotInstanceMethod)table[disposeTitleSceneMethodIndex];
    table[viewChangedMethodIndex] = (uintptr_t)titleSceneViewChanged;
    table[disposeTitleSceneMethodIndex] = (uintptr_t)titleSceneDispose;
    atomic_thread_fence(memory_order_release);
}
// //// /安装精确匹配的 TitleScene AOT 方法表钩子 ////
