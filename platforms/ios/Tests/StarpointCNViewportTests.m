// audience: internal
// # cn-viewport-tests
// iOS Simulator 检查窗口变化, AOT 参数顺序, 页面销毁和输入框坐标.

#include <assert.h>
#include <stdio.h>
#include <string.h>

#import "../PersonalServiceBootstrap/StarpointCNViewportHook.m"

typedef struct { uintptr_t words[192]; } TestObject;
typedef struct { TestObject environment, info; } TestMethod;
static TestObject logic, view, manager, starling, flashLayer, stage, viewport, screen, status;
static TestObject flashMethods, stageMethods, starlingMethods, managerMethods, rectangleMethods, sceneMethods, inputMethods;
static TestObject scene, starlingLayer, flashScene, sceneMaximum, sceneSafe, component, componentMaximum, componentSafe;
static TestObject input, field, region, freshRegion, regionOption, fieldOption, regionParams, fieldParams, arrayMethods;
static TestMethod methods[32];
static ViewportSize windowSize;
static double density = 2;
static int highResolution, contextValid = 1;
static size_t viewportWrites, bufferWrites, nextMethod;

// //// 组装 AVM 对象字段与方法环境 [@x380kkm 2026-09-28] ////
static void putPointer(void *object, size_t offset, void *value) { *(void **)((uint8_t *)object + offset) = value; }
static void bindMethod(TestObject *vtable, size_t slot, void *implementation) {
    TestMethod *method = &methods[nextMethod++];
    assert(nextMethod <= sizeof(methods) / sizeof(methods[0]));
    putPointer(vtable, slot, &method->environment);
    putPointer(&method->environment, methodInfoOffset, &method->info);
    putPointer(&method->info, methodImplementationOffset, implementation);
}
static void setTestRectangle(void *object, void *environment, double x, double y, double width, double height) {
    (void)environment;
    double values[] = {x, y, width, height};
    memcpy((uint8_t *)object + rectangleXOffset, values, sizeof(values));
}
static void *getTestStage(void *object, void *environment) { (void)object; (void)environment; return &stage; }
static int32_t getTestWidth(void *object, void *environment) { (void)object; (void)environment; return (int32_t)windowSize.width; }
static int32_t getTestHeight(void *object, void *environment) { (void)object; (void)environment; return (int32_t)windowSize.height; }
static double getTestDensity(void *object, void *environment) { (void)object; (void)environment; return density; }
static void *getTestViewport(void *object, void *environment) { (void)object; (void)environment; return &viewport; }
static void setTestViewport(void *object, void *rectangle, void *environment) {
    (void)object; (void)environment;
    assert(rectangle == &viewport);
    viewportWrites += 1;
}
static void resizeTestManager(void *object, void *environment) { (void)object; (void)environment; }
static int32_t getTestHighResolution(void *object, void *environment) { (void)object; (void)environment; return highResolution; }
static int32_t getTestContext(void *object, void *environment) { (void)object; (void)environment; return contextValid; }
static void setTestHighResolution(void *object, int32_t value, void *environment) {
    assert(object == &starling && environment == methodEnvironment(object, highResolutionSetter));
    highResolution = value;
    if (contextValid) bufferWrites += 1;
}
static void updateTestBuffer(void *object, int32_t force, void *environment) {
    assert(object == &starling && environment == methodEnvironment(object, updateViewportMethod));
    assert(force == 1 && contextValid);
    bufferWrites += 1;
}
static void setTestX(void *object, void *environment, double value) { (void)environment; writeNumberField(object, 0x400, value); }
static double getTestY(void *object, void *environment) { (void)environment; return *(double *)((uint8_t *)object + 0x408); }
static void setTestY(void *object, void *environment, double value) { (void)environment; writeNumberField(object, 0x408, value); }
static void setTestScaleX(void *object, void *environment, double value) { (void)environment; writeNumberField(object, 0x410, value); }
static void setTestScaleY(void *object, void *environment, double value) { (void)environment; writeNumberField(object, 0x418, value); }
static void prepareTestView(void *object, void *preparation, void *environment) { (void)object; (void)preparation; (void)environment; }
static void disposeTestView(void *object, void *environment) { (void)object; (void)environment; assert(viewLayouts == NULL); }
static void disposeTestScene(void *object, void *environment) { (void)environment; assert(findViewportScene(object) == NULL); }
static void disposeTestInput(void *object, void *environment) { (void)environment; assert(findViewportInput(object) == NULL); }
static uintptr_t getTestArrayElement(void *array, uint32_t index) { assert(index == 0); return (uintptr_t)objectField(array, 0x200) | 1; }
static void *getTestInputRegion(void *object, void *environment) {
    (void)object; (void)environment;
    ViewportTransform transform = sceneViewportTransform(&scene);
    setTestRectangle(&freshRegion, NULL,
        (transform.x - geometry.maximum.x + 80 * transform.scale + 12) * geometry.scale,
        (transform.y - geometry.maximum.y + 120 * transform.scale + 4) * geometry.scale,
        200 * geometry.scale, 80 * geometry.scale);
    return &freshRegion;
}
static void drawTestInput(void *object, int32_t frame, void *environment) {
    (void)object; (void)frame; (void)environment;
    setTestY(&field, NULL, readRectangle(&region).y + 10);
}
// //// /组装 AVM 对象字段与方法环境 ////

// //// 初始化窗口与场景的测试对象 [@x380kkm 2026-09-28] ////
static void prepareObjects(void) {
    putPointer(&logic, logicViewOffset, &view);
    putPointer(&logic, logicStatusOffset, &status);
    putPointer(&view, viewStarlingManagerOffset, &manager);
    putPointer(&view, viewFlashGameLayerOffset, &flashLayer);
    putPointer(&manager, managerScreenSizeOffset, &screen);
    putPointer(&manager, managerStarlingOffset, &starling);
    putPointer(&manager, objectVtableOffset, &managerMethods);
    putPointer(&flashLayer, objectVtableOffset, &flashMethods);
    putPointer(&stage, objectVtableOffset, &stageMethods);
    putPointer(&starling, objectVtableOffset, &starlingMethods);
    TestObject *rectangles[] = {&viewport, &sceneMaximum, &sceneSafe, &componentMaximum, &componentSafe, &region, &freshRegion};
    for (size_t index = 0; index < sizeof(rectangles) / sizeof(rectangles[0]); index += 1)
        putPointer(rectangles[index], objectVtableOffset, &rectangleMethods);
    bindMethod(&flashMethods, displayStageGetter, getTestStage);
    bindMethod(&stageMethods, stageWidthGetter, getTestWidth);
    bindMethod(&stageMethods, stageHeightGetter, getTestHeight);
    bindMethod(&stageMethods, contentsScaleGetter, getTestDensity);
    bindMethod(&starlingMethods, viewportGetter, getTestViewport);
    bindMethod(&starlingMethods, viewportSetter, setTestViewport);
    bindMethod(&starlingMethods, highResolutionGetter, getTestHighResolution);
    bindMethod(&starlingMethods, highResolutionSetter, setTestHighResolution);
    bindMethod(&starlingMethods, contextValidGetter, getTestContext);
    bindMethod(&starlingMethods, updateViewportMethod, updateTestBuffer);
    bindMethod(&managerMethods, managerResizeMethod, resizeTestManager);
    bindMethod(&rectangleMethods, rectangleSetTo, setTestRectangle);
    bindMethod(&flashMethods, displayXSetter, setTestX);
    bindMethod(&flashMethods, displayYSetter, setTestY);
    bindMethod(&flashMethods, displayYGetter, getTestY);
    bindMethod(&flashMethods, displayScaleXSetter, setTestScaleX);
    bindMethod(&flashMethods, displayScaleYSetter, setTestScaleY);
    bindMethod(&sceneMethods, starlingXSetter, setTestX);
    bindMethod(&sceneMethods, starlingYSetter, setTestY);
    bindMethod(&sceneMethods, starlingScaleXSetter, setTestScaleX);
    bindMethod(&sceneMethods, starlingScaleYSetter, setTestScaleY);
    bindMethod(&inputMethods, inputRegionMethod, getTestInputRegion);
    putPointer(&starlingLayer, objectVtableOffset, &sceneMethods);
    putPointer(&flashScene, objectVtableOffset, &flashMethods);
    putPointer(&field, objectVtableOffset, &flashMethods);
    putPointer(&scene, sceneStarlingLayerOffset, &starlingLayer);
    putPointer(&scene, sceneFlashLayerOffset, &flashScene);
    putPointer(&scene, sceneMaximumOffset, &sceneMaximum);
    putPointer(&scene, sceneSafeOffset, &sceneSafe);
    putPointer(&component, viewSceneOffset, &scene);
    putPointer(&component, viewMaximumOffset, &componentMaximum);
    putPointer(&component, viewSafeOffset, &componentSafe);
    putPointer(&input, objectVtableOffset, &inputMethods);
    putPointer(&input, inputSceneOffset, &scene);
    putPointer(&input, inputRegionOffset, &regionOption);
    putPointer(&input, inputTextFieldOffset, &fieldOption);
    putPointer(&regionOption, optionParamsOffset, &regionParams);
    putPointer(&fieldOption, optionParamsOffset, &fieldParams);
    putPointer(&regionParams, 0, &arrayMethods);
    putPointer(&fieldParams, 0, &arrayMethods);
    putPointer(&arrayMethods, arrayGetUintProperty, getTestArrayElement);
    putPointer(&regionParams, 0x200, &region);
    putPointer(&fieldParams, 0x200, &field);
    originalScenePrepare = prepareTestView;
    originalViewPrepare = prepareTestView;
    originalViewDispose = disposeTestView;
    originalSceneDispose = disposeTestScene;
    originalInputRegion = getTestInputRegion;
    originalInputDraw = drawTestInput;
    originalInputDispose = disposeTestInput;
    imageSlide = (uintptr_t)setTestHighResolution - 0x100a7f6a8;
    windowSize = (ViewportSize){1000, 1300};
    synchronizeViewport(&logic);
    writeRectangle(&sceneSafe, geometry.safe);
    writeRectangle(&sceneMaximum, geometry.maximum);
    prepareViewportScene(&scene, NULL, NULL);
    prepareViewportView(&component, NULL, NULL);
    writeRectangle(&region, readRectangle(inputRegion(&input, NULL)));
}
// //// /初始化窗口与场景的测试对象 ////

// //// 检查窗口覆盖与存量控件的安全区域 [@x380kkm 2026-09-28] ////
static void assertContainsRectangle(ViewportRect outer, ViewportRect inner) {
    assert(outer.x <= inner.x + 1e-8 && outer.y <= inner.y + 1e-8);
    assert(outer.x + outer.width >= inner.x + inner.width - 1e-8);
    assert(outer.y + outer.height >= inner.y + inner.height - 1e-8);
}

static void assertGeometry(void) {
    ViewportRect rectangle = readRectangle(&viewport);
    assert(rectangle.x == 0 && rectangle.y == 0);
    assert(rectangle.width >= windowSize.width && rectangle.height >= windowSize.height);
    assert(rectangle.width - windowSize.width < geometry.scale + 1e-8);
    assert(rectangle.height - windowSize.height < geometry.scale + 1e-8);
    assert(fabs(rectangle.width / geometry.stage.width - rectangle.height / geometry.stage.height) < 1e-8);
    ViewportSceneLayout *layout = findViewportScene(&scene);
    ViewportRect localSafe = readRectangle(&sceneSafe);
    assertContainsRectangle(localSafe, layout->content);
    assertContainsRectangle(readRectangle(&sceneMaximum), layout->maximum);
    assert(fabs(readRectangle(&componentSafe).width - localSafe.width) < 1e-8);
    const double *starlingValues = (const double *)((const uint8_t *)&starlingLayer + 0x400);
    const double *flashValues = (const double *)((const uint8_t *)&flashScene + 0x400);
    assert(fabs(flashValues[0] - starlingValues[0] * geometry.scale) < 1e-8);
    assert(fabs(flashValues[2] - starlingValues[2] * geometry.scale) < 1e-8);
}
// //// /检查窗口覆盖与存量控件的安全区域 ////

// //// 覆盖连续分屏变更与渲染上下文恢复 [@x380kkm 2026-09-28] ////
int main(void) {
    @autoreleasepool {
        prepareObjects();
        ViewportSize sizes[] = {{500, 900}, {1600, 900}, {320, 1000}, {1000, 1300}, {500, 900}};
        for (size_t index = 0; index < sizeof(sizes) / sizeof(sizes[0]); index += 1) {
            windowSize = sizes[index];
            synchronizeViewport(&logic);
            assertGeometry();
            drawInput(&input, 0, NULL);
            double firstY = getTestY(&field, NULL);
            for (int frame = 1; frame < 50; frame += 1) drawInput(&input, frame, NULL);
            assert(fabs(getTestY(&field, NULL) - firstY) < 1e-8);
        }
        size_t writes = viewportWrites, buffers = bufferWrites;
        synchronizeViewport(&logic);
        assert(viewportWrites == writes && bufferWrites == buffers);
        density = 3;
        synchronizeViewport(&logic);
        assert(bufferWrites == buffers + 1);
        contextValid = 0;
        windowSize = (ViewportSize){600, 800};
        synchronizeViewport(&logic);
        assert(bufferWrites == buffers + 1);
        contextValid = 1;
        synchronizeViewport(&logic);
        assert(highResolution == 1 && bufferWrites == buffers + 2);
        windowSize = (ViewportSize){0, 0};
        synchronizeViewport(&logic);
        assert(geometry.window.width == 600);
        ViewportGeometry inset = makeViewportGeometry((ViewportSize){768, 1024}, (ViewportInsets){24, 8, 20, 12}, 2);
        assert(inset.valid && inset.safe.width >= 1080 && inset.safe.height >= 1920);
        assert(fabs((inset.safe.x - inset.maximum.x) * inset.scale - 8) < 1e-8);
        assert(!makeViewportGeometry((ViewportSize){NAN, 1024}, (ViewportInsets){0}, 2).valid);
        ViewportGeometry leadingInset = makeViewportGeometry((ViewportSize){768, 1024}, (ViewportInsets){150, 140, 5, 4}, 2);
        ViewportGeometry trailingInset = makeViewportGeometry((ViewportSize){768, 1024}, (ViewportInsets){5, 4, 150, 140}, 2);
        ViewportTransform insetTransform = fitViewportLayout(leadingInset.maximum, leadingInset.safe,
                                                             trailingInset.maximum, trailingInset.safe);
        assertContainsRectangle(viewportRectInContent(trailingInset.maximum, insetTransform), leadingInset.maximum);
        assertContainsRectangle(viewportRectInContent(trailingInset.safe, insetTransform), leadingInset.safe);
        disposeViewportInput(&input, NULL);
        disposeViewportView(&component, NULL);
        disposeViewportScene(&scene, NULL);
        memset(&scene, 0xcc, sizeof(scene));
        memset(&component, 0xcc, sizeof(component));
        refreshViewportLayouts();
        puts("窗口几何, 输入坐标和页面生命周期检查通过.");
    }
    return 0;
}
// //// /覆盖连续分屏变更与渲染上下文恢复 ////
