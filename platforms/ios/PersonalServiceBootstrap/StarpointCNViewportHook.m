// audience: internal
// # cn-viewport-hook
// iPad 的 AIR 舞台使用当前窗口尺寸, Starling 在有效渲染上下文中配置屏幕像素密度.
// AOT 对象在游戏帧内访问, 页面几何记录随 Gear 的准备和销毁回调创建与释放.

#import <UIKit/UIKit.h>

#include <stdatomic.h>
#include <stdlib.h>

#import "StarpointCNAotRuntime.h"
#include "StarpointCNViewportGeometry.h"

enum {
    logicUpdateIndex = 29153, screenResetIndex = 33896, statusResetIndex = 33582,
    safeAreaIndex = 31509, managerViewportIndex = 51653,
    viewPrepareIndex = 34944, viewDisposeIndex = 34945,
    scenePrepareIndex = 34515, sceneDisposeIndex = 34527,
    inputRegionIndex = 93675, inputDrawIndex = 93681, inputDisposeIndex = 93682,
    objectVtableOffset = 0x10, methodInfoOffset = 0x10, methodImplementationOffset = 0x50,
    logicViewOffset = 0x30, logicStatusOffset = 0xa8,
    viewStarlingManagerOffset = 0x90, viewFlashGameLayerOffset = 0xf0,
    managerScreenSizeOffset = 0x38, managerStarlingOffset = 0x40, managerResizeMethod = 0xb8,
    displayStageGetter = 0xf0, displayXSetter = 0x148, displayYGetter = 0x150,
    displayYSetter = 0x158, displayScaleXSetter = 0x178, displayScaleYSetter = 0x188,
    stageWidthGetter = 0x540, stageHeightGetter = 0x550, contentsScaleGetter = 0x688,
    viewportGetter = 0x220, viewportSetter = 0x228,
    highResolutionGetter = 0x2d8, highResolutionSetter = 0x2e0,
    contextValidGetter = 0x328, updateViewportMethod = 0x108,
    rectangleSetTo = 0x1a8, rectangleXOffset = 0x20, rectangleYOffset = 0x28,
    rectangleWidthOffset = 0x30, rectangleHeightOffset = 0x38,
    inputRegionOffset = 0x40, inputTextFieldOffset = 0xb0,
    optionIndexOffset = 0x20, optionParamsOffset = 0x30, arrayGetUintProperty = 0x90,
};

typedef void (*InstanceMethod)(void *, void *);
typedef void (*PreparationMethod)(void *, void *, void *);
typedef void *(*ObjectGetter)(void *, void *);
typedef void (*ObjectSetter)(void *, void *, void *);
typedef int32_t (*IntegerGetter)(void *, void *);
typedef void (*IntegerSetter)(void *, int32_t, void *);
typedef double (*NumberGetter)(void *, void *);
typedef void (*NumberSetter)(void *, void *, double);
typedef void (*RectangleSetter)(void *, void *, double, double, double, double);
typedef void (*DrawMethod)(void *, int32_t, void *);
typedef uintptr_t (*ArrayElementGetter)(void *, uint32_t);

static ViewportGeometry geometry;
static InstanceMethod originalLogicUpdate, originalScreenReset, originalStatusReset;
static ObjectGetter originalSafeArea, originalManagerViewport, originalInputRegion;
static DrawMethod originalInputDraw;
static uintptr_t imageSlide;
static uint64_t geometryRevision;
static BOOL resolutionPending;

// //// 读取经版本验证的 AVM 对象字段与虚方法 [@x380kkm 2026-09-28] ////
static void *objectField(void *object, size_t offset) {
    return object == NULL ? NULL : *(void **)((uint8_t *)object + offset);
}

static void *methodEnvironment(void *object, size_t slot) {
    return objectField(objectField(object, objectVtableOffset), slot);
}

static void *methodImplementation(void *environment) {
    return objectField(objectField(environment, methodInfoOffset), methodImplementationOffset);
}

static void *getObject(void *object, size_t slot) {
    void *environment = methodEnvironment(object, slot);
    return ((ObjectGetter)methodImplementation(environment))(object, environment);
}

static int32_t getInteger(void *object, size_t slot) {
    void *environment = methodEnvironment(object, slot);
    return ((IntegerGetter)methodImplementation(environment))(object, environment);
}

static double getNumber(void *object, size_t slot) {
    void *environment = methodEnvironment(object, slot);
    return ((NumberGetter)methodImplementation(environment))(object, environment);
}

static void setNumber(void *object, size_t slot, double value) {
    void *environment = methodEnvironment(object, slot);
    ((NumberSetter)methodImplementation(environment))(object, environment, value);
}

static void setInteger(void *object, size_t slot, int32_t value) {
    void *environment = methodEnvironment(object, slot);
    ((IntegerSetter)methodImplementation(environment))(object, value, environment);
}

static void writeNumberField(void *object, size_t offset, double value) {
    *(double *)((uint8_t *)object + offset) = value;
}
// //// /读取经版本验证的 AVM 对象字段与虚方法 ////

// //// 在 Flash 矩形与原生几何值之间传递坐标 [@x380kkm 2026-09-28] ////
static ViewportRect readRectangle(void *rectangle) {
    const double *values = (const double *)((const uint8_t *)rectangle + rectangleXOffset);
    return (ViewportRect){values[0], values[1], values[2], values[3]};
}

static void writeRectangle(void *rectangle, ViewportRect value) {
    if (rectangle == NULL) return;
    void *environment = methodEnvironment(rectangle, rectangleSetTo);
    ((RectangleSetter)methodImplementation(environment))(
        rectangle, environment, value.x, value.y, value.width, value.height
    );
}

static BOOL hasValidSize(ViewportRect rectangle) {
    return isfinite(rectangle.x) && isfinite(rectangle.y) && isfinite(rectangle.width) &&
           isfinite(rectangle.height) && rectangle.width > 0 && rectangle.height > 0;
}
// //// /在 Flash 矩形与原生几何值之间传递坐标 ////

#include "StarpointCNViewportLayout.h"

// //// 将窗口几何写入游戏屏幕数据与逻辑状态 [@x380kkm 2026-09-28] ////
static void applyScreenSize(void *screen) {
    if (screen == NULL || !geometry.valid) return;
    *(int32_t *)((uint8_t *)screen + 0x28) = (int32_t)geometry.window.width;
    *(int32_t *)((uint8_t *)screen + 0x34) = (int32_t)geometry.window.height;
    *(int32_t *)((uint8_t *)screen + 0x30) = 0;
    *(int32_t *)((uint8_t *)screen + 0x2c) = 0;
    *(int32_t *)((uint8_t *)screen + 0x38) = geometry.window.width / geometry.window.height < 0.49;
    writeNumberField(screen, 0x70, -geometry.maximum.x * geometry.scale);
    writeNumberField(screen, 0x68, -geometry.maximum.y * geometry.scale);
    writeNumberField(screen, 0x50, geometry.scale);
    writeNumberField(screen, 0x58, geometry.insets.top / geometry.scale);
    writeNumberField(screen, 0x60, geometry.insets.bottom / geometry.scale);
}

static void applyLogicSize(void *status) {
    if (status == NULL || !geometry.valid) return;
    writeNumberField(status, 0x1d0, -geometry.maximum.x);
    writeNumberField(status, 0x1c8, -geometry.maximum.y);
    writeNumberField(status, 0x1d8, geometry.insets.top / geometry.scale);
    writeNumberField(status, 0x1e0, geometry.insets.bottom / geometry.scale);
}

static void resetScreenSize(void *receiver, void *environment) {
    originalScreenReset(receiver, environment);
    applyScreenSize(receiver);
}

static void resetLogicSize(void *receiver, void *environment) {
    originalStatusReset(receiver, environment);
    applyLogicSize(receiver);
}

static void *getSafeArea(void *receiver, void *environment) {
    void *rectangle = originalSafeArea(receiver, environment);
    if (geometry.valid) writeRectangle(rectangle, geometry.safe);
    return rectangle;
}

static void *getManagerViewport(void *receiver, void *environment) {
    void *rectangle = originalManagerViewport(receiver, environment);
    if (geometry.valid) writeRectangle(rectangle, geometry.viewport);
    return rectangle;
}
// //// /将窗口几何写入游戏屏幕数据与逻辑状态 ////

// //// 在主线程读取当前应用窗口的安全区域 [@x380kkm 2026-09-28] ////
static UIWindow *gameWindow(void) {
    UIApplication *application = UIApplication.sharedApplication;
    if (@available(iOS 13.0, *)) {
        for (UIScene *scene in application.connectedScenes) {
            if (![scene isKindOfClass:UIWindowScene.class] ||
                scene.activationState == UISceneActivationStateBackground ||
                scene.activationState == UISceneActivationStateUnattached) continue;
            for (UIWindow *window in ((UIWindowScene *)scene).windows) {
                if (window.isKeyWindow && window.windowLevel == UIWindowLevelNormal) return window;
            }
        }
    }
    id<UIApplicationDelegate> delegate = application.delegate;
    if ([delegate respondsToSelector:@selector(window)]) {
        UIWindow *window = delegate.window;
        if (!window.hidden && window.windowLevel == UIWindowLevelNormal) return window;
    }
    return nil;
}

static ViewportInsets windowInsets(UIWindow *window, ViewportSize size) {
    CGSize bounds = window.bounds.size;
    if (bounds.width <= 0 || bounds.height <= 0) return (ViewportInsets){0};
    UIEdgeInsets insets = window.safeAreaInsets;
    return (ViewportInsets){insets.top * size.height / bounds.height,
                             insets.left * size.width / bounds.width,
                             insets.bottom * size.height / bounds.height,
                             insets.right * size.width / bounds.width};
}
// //// /在主线程读取当前应用窗口的安全区域 ////

// //// 在有效 Starling 上下文中刷新屏幕像素密度 [@x380kkm 2026-09-28] ////
static void synchronizeResolution(void *starling, BOOL changed) {
    if ((uintptr_t)methodImplementation(methodEnvironment(starling, highResolutionSetter)) !=
        imageSlide + 0x100a7f6a8) return;
    resolutionPending |= changed;
    if (!getInteger(starling, highResolutionGetter)) {
        setInteger(starling, highResolutionSetter, 1);
        resolutionPending = !getInteger(starling, contextValidGetter);
    } else if (resolutionPending && getInteger(starling, contextValidGetter)) {
        setInteger(starling, updateViewportMethod, 1);
        resolutionPending = NO;
    }
}
// //// /在有效 Starling 上下文中刷新屏幕像素密度 ////

// //// 在游戏帧中同步窗口舞台和存量页面几何 [@x380kkm 2026-09-28] ////
static void synchronizeViewport(void *logic) {
    if (!NSThread.isMainThread) return;
    void *view = objectField(logic, logicViewOffset);
    void *manager = objectField(view, viewStarlingManagerOffset);
    void *starling = objectField(manager, managerStarlingOffset);
    void *flashLayer = objectField(view, viewFlashGameLayerOffset);
    if (starling == NULL || flashLayer == NULL) return;
    void *stage = getObject(flashLayer, displayStageGetter);
    if (stage == NULL) return;
    ViewportSize size = {getInteger(stage, stageWidthGetter), getInteger(stage, stageHeightGetter)};
    UIWindow *window = gameWindow();
    ViewportGeometry next = makeViewportGeometry(size, windowInsets(window, size),
                                                 getNumber(stage, contentsScaleGetter));
    if (!next.valid) return;
    void *rectangle = getObject(starling, viewportGetter);
    if (rectangle == NULL) return;
    ViewportRect current = readRectangle(rectangle);
    BOOL changed = !geometry.valid || geometry.window.width != size.width || geometry.window.height != size.height ||
        geometry.contentsScale != next.contentsScale || !CGRectEqualToRect(
            CGRectMake(geometry.safe.x, geometry.safe.y, geometry.safe.width, geometry.safe.height),
            CGRectMake(next.safe.x, next.safe.y, next.safe.width, next.safe.height));
    BOOL viewportChanged = current.x != next.viewport.x || current.y != next.viewport.y ||
        current.width != next.viewport.width || current.height != next.viewport.height;
    geometry = next;
    if (changed || viewportChanged) {
        geometryRevision += 1;
        applyScreenSize(objectField(manager, managerScreenSizeOffset));
        applyLogicSize(objectField(logic, logicStatusOffset));
        void *environment = methodEnvironment(manager, managerResizeMethod);
        ((InstanceMethod)methodImplementation(environment))(manager, environment);
        writeRectangle(rectangle, geometry.viewport);
        environment = methodEnvironment(starling, viewportSetter);
        ((ObjectSetter)methodImplementation(environment))(starling, rectangle, environment);
        refreshViewportLayouts();
    }
    synchronizeResolution(starling, changed || viewportChanged);
}

static void updateLogic(void *receiver, void *environment) {
    synchronizeViewport(receiver);
    originalLogicUpdate(receiver, environment);
}
// //// /在游戏帧中同步窗口舞台和存量页面几何 ////

#include "StarpointCNViewportInput.h"

// //// 为 iPad 安装精确匹配的游戏尺寸适配 [@x380kkm 2026-09-28] ////
__attribute__((constructor)) static void installCnViewportHook(void) {
    const StarpointCnMethodSignature signatures[] = {
        {logicUpdateIndex, 0x104c76d54}, {screenResetIndex, 0x1050463d0},
        {statusResetIndex, 0x105033b48}, {safeAreaIndex, 0x104f8d7fc},
        {managerViewportIndex, 0x1021148ac}, {viewPrepareIndex, 0x103bacca0},
        {viewDisposeIndex, 0x103bacb58}, {scenePrepareIndex, 0x1038617a4},
        {sceneDisposeIndex, 0x10385fe30}, {inputRegionIndex, 0x1038fe21c},
        {inputDrawIndex, 0x1038fd534}, {inputDisposeIndex, 0x1038fd1a8},
    };
    uintptr_t *table = starpointCnValidatedMethodTable(signatures, sizeof(signatures) / sizeof(signatures[0]));
    if (table == NULL || UIDevice.currentDevice.userInterfaceIdiom != UIUserInterfaceIdiomPad) return;
    imageSlide = table[logicUpdateIndex] - 0x104c76d54;
    originalLogicUpdate = (InstanceMethod)table[logicUpdateIndex];
    originalScreenReset = (InstanceMethod)table[screenResetIndex];
    originalStatusReset = (InstanceMethod)table[statusResetIndex];
    originalSafeArea = (ObjectGetter)table[safeAreaIndex];
    originalManagerViewport = (ObjectGetter)table[managerViewportIndex];
    originalViewPrepare = (PreparationMethod)table[viewPrepareIndex];
    originalViewDispose = (InstanceMethod)table[viewDisposeIndex];
    originalScenePrepare = (PreparationMethod)table[scenePrepareIndex];
    originalSceneDispose = (InstanceMethod)table[sceneDisposeIndex];
    originalInputRegion = (ObjectGetter)table[inputRegionIndex];
    originalInputDraw = (DrawMethod)table[inputDrawIndex];
    originalInputDispose = (InstanceMethod)table[inputDisposeIndex];
    table[logicUpdateIndex] = (uintptr_t)updateLogic;
    table[screenResetIndex] = (uintptr_t)resetScreenSize;
    table[statusResetIndex] = (uintptr_t)resetLogicSize;
    table[safeAreaIndex] = (uintptr_t)getSafeArea;
    table[managerViewportIndex] = (uintptr_t)getManagerViewport;
    table[viewPrepareIndex] = (uintptr_t)prepareViewportView;
    table[viewDisposeIndex] = (uintptr_t)disposeViewportView;
    table[scenePrepareIndex] = (uintptr_t)prepareViewportScene;
    table[sceneDisposeIndex] = (uintptr_t)disposeViewportScene;
    table[inputRegionIndex] = (uintptr_t)inputRegion;
    table[inputDrawIndex] = (uintptr_t)drawInput;
    table[inputDisposeIndex] = (uintptr_t)disposeViewportInput;
    atomic_thread_fence(memory_order_release);
}
// //// /为 iPad 安装精确匹配的游戏尺寸适配 ////
