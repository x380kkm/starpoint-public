// audience: internal
// # cn-viewport-geometry
// 场景使用 1080 x 1920 的内容坐标, 可见范围按窗口比例扩展.
// 视口和逻辑舞台使用相同的缩放系数, 舞台整数取整由渲染边界吸收.

#ifndef STARPOINT_CN_VIEWPORT_GEOMETRY_H
#define STARPOINT_CN_VIEWPORT_GEOMETRY_H

#include <math.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct { double width, height; } ViewportSize;
typedef struct { double x, y, width, height; } ViewportRect;
typedef struct { double top, left, bottom, right; } ViewportInsets;
typedef struct {
    bool valid;
    ViewportSize window;
    ViewportSize stage;
    ViewportRect viewport;
    ViewportRect maximum;
    ViewportRect safe;
    ViewportInsets insets;
    double scale;
    double contentsScale;
} ViewportGeometry;

typedef struct { double x, y, scale; } ViewportTransform;

// //// 计算覆盖窗口的等比舞台与安全区域 [@x380kkm 2026-09-28] ////
static ViewportGeometry makeViewportGeometry(ViewportSize window, ViewportInsets insets, double contentsScale) {
    ViewportGeometry result = {0};
    double availableWidth = window.width - insets.left - insets.right;
    double availableHeight = window.height - insets.top - insets.bottom;
    if (!isfinite(window.width) || !isfinite(window.height) ||
        !isfinite(availableWidth) || !isfinite(availableHeight) ||
        !isfinite(contentsScale) || contentsScale <= 0 ||
        insets.left < 0 || insets.top < 0 || insets.right < 0 || insets.bottom < 0 ||
        window.width < 32 || window.height < 32 || availableWidth <= 0 || availableHeight <= 0) {
        return result;
    }
    double scale = fmin(availableWidth / 1080.0, availableHeight / 1920.0);
    double width = ceil(window.width / scale);
    double height = ceil(window.height / scale);
    if (width > INT32_MAX || height > INT32_MAX) {
        return result;
    }
    double marginX = (width - 1080.0) * 0.5;
    double marginY = (height - 1920.0) * 0.5;
    result.valid = true;
    result.window = window;
    result.stage = (ViewportSize){width, height};
    result.viewport = (ViewportRect){0, 0, width * scale, height * scale};
    result.maximum = (ViewportRect){-marginX, -marginY, width, height};
    result.safe = (ViewportRect){insets.left / scale - marginX, insets.top / scale - marginY,
                                 availableWidth / scale, availableHeight / scale};
    result.insets = insets;
    result.scale = scale;
    result.contentsScale = contentsScale;
    return result;
}
// //// /计算覆盖窗口的等比舞台与安全区域 ////

// //// 将窗口范围换算到场景本地坐标 [@x380kkm 2026-09-28] ////
static ViewportRect viewportRectInContent(ViewportRect rectangle, ViewportTransform transform) {
    return (ViewportRect){(rectangle.x - transform.x) / transform.scale,
                          (rectangle.y - transform.y) / transform.scale,
                          rectangle.width / transform.scale, rectangle.height / transform.scale};
}
// //// /将窗口范围换算到场景本地坐标 ////

// //// 同时保全场景边界与安全区域内的控件 [@x380kkm 2026-09-28] ////
static ViewportTransform fitViewportLayout(ViewportRect sourceMaximum, ViewportRect sourceSafe,
                                          ViewportRect targetMaximum, ViewportRect targetSafe) {
    ViewportRect sources[] = {sourceMaximum, sourceSafe};
    ViewportRect targets[] = {targetMaximum, targetSafe};
    double scale = INFINITY;
    for (size_t lower = 0; lower < 2; lower += 1) {
        for (size_t upper = 0; upper < 2; upper += 1) {
            scale = fmin(scale, (targets[upper].x + targets[upper].width - targets[lower].x) /
                               (sources[upper].x + sources[upper].width - sources[lower].x));
            scale = fmin(scale, (targets[upper].y + targets[upper].height - targets[lower].y) /
                               (sources[upper].y + sources[upper].height - sources[lower].y));
        }
    }
    double minimumX = fmax(targetMaximum.x - sourceMaximum.x * scale, targetSafe.x - sourceSafe.x * scale);
    double maximumX = fmin(targetMaximum.x + targetMaximum.width - (sourceMaximum.x + sourceMaximum.width) * scale,
                          targetSafe.x + targetSafe.width - (sourceSafe.x + sourceSafe.width) * scale);
    double minimumY = fmax(targetMaximum.y - sourceMaximum.y * scale, targetSafe.y - sourceSafe.y * scale);
    double maximumY = fmin(targetMaximum.y + targetMaximum.height - (sourceMaximum.y + sourceMaximum.height) * scale,
                          targetSafe.y + targetSafe.height - (sourceSafe.y + sourceSafe.height) * scale);
    double x = targetSafe.x + targetSafe.width * 0.5 - (sourceSafe.x + sourceSafe.width * 0.5) * scale;
    double y = targetSafe.y + targetSafe.height * 0.5 - (sourceSafe.y + sourceSafe.height * 0.5) * scale;
    return (ViewportTransform){fmax(minimumX, fmin(maximumX, x)), fmax(minimumY, fmin(maximumY, y)), scale};
}
// //// /同时保全场景边界与安全区域内的控件 ////

#endif
