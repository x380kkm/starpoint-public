// audience: internal
// # cn-viewport-layout
// Gear 持有页面对象至销毁回调, 几何记录在对应回调调用原方法之前移除.
// 存量场景保留控件的相对布局, 其几何缓存使用当前窗口在场景本地坐标中的范围.

typedef struct ViewportSceneLayout {
    void *receiver;
    ViewportRect content;
    ViewportRect maximum;
    ViewportTransform transform;
    struct ViewportSceneLayout *next;
} ViewportSceneLayout;

typedef struct ViewportViewLayout {
    void *receiver;
    struct ViewportViewLayout *next;
} ViewportViewLayout;

static ViewportSceneLayout *sceneLayouts;
static ViewportViewLayout *viewLayouts;
static PreparationMethod originalScenePrepare, originalViewPrepare;
static InstanceMethod originalSceneDispose, originalViewDispose;

enum {
    sceneStarlingLayerOffset = 0x50, sceneFlashLayerOffset = 0xa8,
    sceneSafeOffset = 0x70, sceneMaximumOffset = 0x78,
    viewSafeOffset = 0x30, viewMaximumOffset = 0x38, viewSceneOffset = 0x40,
    starlingXSetter = 0x228, starlingYSetter = 0x238,
    starlingScaleXSetter = 0x268, starlingScaleYSetter = 0x278,
};

// //// 查找存量页面的坐标映射 [@x380kkm 2026-09-28] ////
static ViewportSceneLayout *findViewportScene(void *receiver) {
    for (ViewportSceneLayout *layout = sceneLayouts; layout != NULL; layout = layout->next) {
        if (layout->receiver == receiver) return layout;
    }
    return NULL;
}

static ViewportTransform sceneViewportTransform(void *receiver) {
    ViewportSceneLayout *layout = findViewportScene(receiver);
    return layout == NULL ? (ViewportTransform){0, 0, 1} : layout->transform;
}
// //// /查找存量页面的坐标映射 ////

// //// 将 Flash 和 Starling 场景放置在同一窗口坐标中 [@x380kkm 2026-09-28] ////
static void applySceneLayout(ViewportSceneLayout *layout) {
    if (!geometry.valid) return;
    layout->transform = fitViewportLayout(layout->maximum, layout->content, geometry.maximum, geometry.safe);
    ViewportTransform transform = layout->transform;
    double x = transform.x - geometry.maximum.x;
    double y = transform.y - geometry.maximum.y;
    void *layer = objectField(layout->receiver, sceneStarlingLayerOffset);
    if (layer != NULL) {
        setNumber(layer, starlingXSetter, x);
        setNumber(layer, starlingYSetter, y);
        setNumber(layer, starlingScaleXSetter, transform.scale);
        setNumber(layer, starlingScaleYSetter, transform.scale);
    }
    layer = objectField(layout->receiver, sceneFlashLayerOffset);
    if (layer != NULL) {
        setNumber(layer, displayXSetter, x * geometry.scale);
        setNumber(layer, displayYSetter, y * geometry.scale);
        setNumber(layer, displayScaleXSetter, transform.scale * geometry.scale);
        setNumber(layer, displayScaleYSetter, transform.scale * geometry.scale);
    }
    writeRectangle(objectField(layout->receiver, sceneMaximumOffset),
                   viewportRectInContent(geometry.maximum, transform));
    writeRectangle(objectField(layout->receiver, sceneSafeOffset),
                   viewportRectInContent(geometry.safe, transform));
}

static void applyViewLayout(void *receiver) {
    if (!geometry.valid) return;
    ViewportTransform transform = sceneViewportTransform(objectField(receiver, viewSceneOffset));
    writeRectangle(objectField(receiver, viewMaximumOffset), viewportRectInContent(geometry.maximum, transform));
    writeRectangle(objectField(receiver, viewSafeOffset), viewportRectInContent(geometry.safe, transform));
}

static void refreshViewportLayouts(void) {
    for (ViewportSceneLayout *layout = sceneLayouts; layout != NULL; layout = layout->next) applySceneLayout(layout);
    for (ViewportViewLayout *layout = viewLayouts; layout != NULL; layout = layout->next) applyViewLayout(layout->receiver);
}
// //// /将 Flash 和 Starling 场景放置在同一窗口坐标中 ////

// //// 在页面准备结束时记录存量控件区域 [@x380kkm 2026-09-28] ////
static void prepareViewportScene(void *receiver, void *preparation, void *environment) {
    originalScenePrepare(receiver, preparation, environment);
    void *safe = objectField(receiver, sceneSafeOffset);
    void *maximum = objectField(receiver, sceneMaximumOffset);
    if (safe == NULL || maximum == NULL || !hasValidSize(readRectangle(safe)) || !hasValidSize(readRectangle(maximum))) return;
    ViewportSceneLayout *layout = findViewportScene(receiver);
    if (layout == NULL) {
        layout = calloc(1, sizeof(*layout));
        if (layout == NULL) return;
        layout->receiver = receiver;
        layout->next = sceneLayouts;
        sceneLayouts = layout;
    }
    layout->content = readRectangle(safe);
    layout->maximum = readRectangle(maximum);
    layout->transform = (ViewportTransform){0, 0, 1};
    applySceneLayout(layout);
}

static void prepareViewportView(void *receiver, void *preparation, void *environment) {
    originalViewPrepare(receiver, preparation, environment);
    for (ViewportViewLayout *layout = viewLayouts; layout != NULL; layout = layout->next) {
        if (layout->receiver == receiver) {
            applyViewLayout(receiver);
            return;
        }
    }
    ViewportViewLayout *layout = calloc(1, sizeof(*layout));
    if (layout == NULL) return;
    *layout = (ViewportViewLayout){receiver, viewLayouts};
    viewLayouts = layout;
    applyViewLayout(receiver);
}
// //// /在页面准备结束时记录存量控件区域 ////

// //// 在页面归还矩形前释放坐标记录 [@x380kkm 2026-09-28] ////
static void disposeViewportView(void *receiver, void *environment) {
    ViewportViewLayout **entry = &viewLayouts;
    while (*entry != NULL) {
        if ((*entry)->receiver == receiver) {
            ViewportViewLayout *removed = *entry;
            *entry = removed->next;
            free(removed);
            break;
        }
        entry = &(*entry)->next;
    }
    originalViewDispose(receiver, environment);
}

static void disposeViewportScene(void *receiver, void *environment) {
    ViewportSceneLayout **entry = &sceneLayouts;
    while (*entry != NULL) {
        if ((*entry)->receiver == receiver) {
            ViewportSceneLayout *removed = *entry;
            *entry = removed->next;
            free(removed);
            break;
        }
        entry = &(*entry)->next;
    }
    originalSceneDispose(receiver, environment);
}
// //// /在页面归还矩形前释放坐标记录 ////
