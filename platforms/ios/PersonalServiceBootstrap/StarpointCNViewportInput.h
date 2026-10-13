// audience: internal
// # cn-viewport-input
// Stage 直系输入框使用创建时的字体尺寸, 显示变换与所属场景的当前窗口映射一致.
// 输入框记录在 disposeFlashTextField 调用原方法之前释放.

typedef struct ViewportInputLayout {
    void *receiver;
    double fontScale;
    uint64_t revision;
    struct ViewportInputLayout *next;
} ViewportInputLayout;

static ViewportInputLayout *inputLayouts;
static InstanceMethod originalInputDispose;
enum { inputSceneOffset = 0x98, inputRegionMethod = 0x160 };

// //// 从 Haxe Some 中读取 Flash 对象 [@x380kkm 2026-09-28] ////
static void *optionObject(void *option) {
    if (option == NULL || *(int32_t *)((uint8_t *)option + optionIndexOffset) != 0) return NULL;
    void *array = objectField(option, optionParamsOffset);
    if (array == NULL) return NULL;
    void *nativeVtable = objectField(array, 0);
    ArrayElementGetter getter = (ArrayElementGetter)objectField(nativeVtable, arrayGetUintProperty);
    uintptr_t atom = getter(array, 0);
    return (atom & 7) == 1 ? (void *)(atom & ~(uintptr_t)7) : NULL;
}
// //// /从 Haxe Some 中读取 Flash 对象 ////

// //// 将输入区域换算到字体创建时的坐标系 [@x380kkm 2026-09-28] ////
static ViewportInputLayout *findViewportInput(void *receiver) {
    for (ViewportInputLayout *layout = inputLayouts; layout != NULL; layout = layout->next) {
        if (layout->receiver == receiver) return layout;
    }
    return NULL;
}

static double inputDisplayScale(ViewportInputLayout *layout) {
    ViewportTransform scene = sceneViewportTransform(objectField(layout->receiver, inputSceneOffset));
    return geometry.scale * scene.scale / layout->fontScale;
}

static ViewportRect normalizeInputRegion(ViewportInputLayout *layout, ViewportRect rectangle) {
    ViewportTransform scene = sceneViewportTransform(objectField(layout->receiver, inputSceneOffset));
    double scale = inputDisplayScale(layout);
    rectangle.x += 12 * (scene.scale - 1) * geometry.scale;
    rectangle.y += 4 * (scene.scale - 1) * geometry.scale;
    return (ViewportRect){rectangle.x / scale, rectangle.y / scale,
                          rectangle.width * scene.scale / scale, rectangle.height * scene.scale / scale};
}

static void *inputRegion(void *receiver, void *environment) {
    void *rectangle = originalInputRegion(receiver, environment);
    if (rectangle == NULL || !geometry.valid) return rectangle;
    ViewportInputLayout *layout = findViewportInput(receiver);
    if (layout == NULL) {
        layout = calloc(1, sizeof(*layout));
        if (layout == NULL) return rectangle;
        *layout = (ViewportInputLayout){receiver, geometry.scale, geometryRevision, inputLayouts};
        inputLayouts = layout;
    }
    writeRectangle(rectangle, normalizeInputRegion(layout, readRectangle(rectangle)));
    layout->revision = geometryRevision;
    return rectangle;
}
// //// /将输入区域换算到字体创建时的坐标系 ////

// //// 在窗口变化后同步输入框与文字基线 [@x380kkm 2026-09-28] ////
static void drawInput(void *receiver, int32_t frame, void *environment) {
    ViewportInputLayout *layout = findViewportInput(receiver);
    void *field = optionObject(objectField(receiver, inputTextFieldOffset));
    void *rectangle = optionObject(objectField(receiver, inputRegionOffset));
    if (layout != NULL && field != NULL && rectangle != NULL && geometry.valid &&
        layout->revision != geometryRevision) {
        void *regionEnvironment = methodEnvironment(receiver, inputRegionMethod);
        void *current = originalInputRegion(receiver, regionEnvironment);
        if (current != NULL) writeRectangle(rectangle, normalizeInputRegion(layout, readRectangle(current)));
        layout->revision = geometryRevision;
    }
    originalInputDraw(receiver, frame, environment);
    if (layout == NULL || field == NULL || rectangle == NULL || !geometry.valid) return;
    double scale = inputDisplayScale(layout);
    setNumber(field, displayXSetter, readRectangle(rectangle).x * scale);
    setNumber(field, displayYSetter, getNumber(field, displayYGetter) * scale);
    setNumber(field, displayScaleXSetter, scale);
    setNumber(field, displayScaleYSetter, scale);
}
// //// /在窗口变化后同步输入框与文字基线 ////

// //// 在输入框离开 Stage 前移除缩放记录 [@x380kkm 2026-09-28] ////
static void disposeViewportInput(void *receiver, void *environment) {
    ViewportInputLayout **entry = &inputLayouts;
    while (*entry != NULL) {
        if ((*entry)->receiver == receiver) {
            ViewportInputLayout *removed = *entry;
            *entry = removed->next;
            free(removed);
            break;
        }
        entry = &(*entry)->next;
    }
    originalInputDispose(receiver, environment);
}
// //// /在输入框离开 Stage 前移除缩放记录 ////
