// audience: external
// # android-personal-service-jni
//
// 该文件把伴随应用 JNI 调用转换为个人服务 C ABI. Java 层拥有并串行释放句柄.

#include <jni.h>
#include <stdint.h>

#include "starpoint_personal_service.h"

// //// 转换 Java 句柄和 C 服务指针 [@x380kkm 2026-08-31] ////
static StarpointPersonalService *service_from_handle(jlong handle) {
    return (StarpointPersonalService *)(uintptr_t)handle;
}

static void throw_exception(JNIEnv *env, const char *class_name, const char *message) {
    jclass exception_class = (*env)->FindClass(env, class_name);
    if (exception_class != NULL) {
        (*env)->ThrowNew(env, exception_class, message);
    }
}
// //// /转换 Java 句柄和 C 服务指针 ////

// //// 转换伴随应用个人服务生命周期 [@x380kkm 2026-08-31] ////
JNIEXPORT jlong JNICALL Java_dev_starpoint_personalservice_CompanionServiceHost_nativeStart(
    JNIEnv *env,
    jclass host_class,
    jstring root_path,
    jstring cn_asset_root,
    jint requested_port
) {
    (void)host_class;
    if (
        root_path == NULL
            || cn_asset_root == NULL
            || requested_port != 17171
    ) {
        throw_exception(env, "java/lang/IllegalArgumentException", "Invalid companion service start arguments.");
        return 0;
    }

    const char *root_path_bytes = (*env)->GetStringUTFChars(env, root_path, NULL);
    if (root_path_bytes == NULL) {
        return 0;
    }
    const char *cn_asset_root_bytes = (*env)->GetStringUTFChars(env, cn_asset_root, NULL);
    if (cn_asset_root_bytes == NULL) {
        (*env)->ReleaseStringUTFChars(env, root_path, root_path_bytes);
        return 0;
    }
    StarpointPersonalService *service = starpoint_personal_service_start_with_cdn_root(
        root_path_bytes,
        cn_asset_root_bytes,
        (uint16_t)requested_port
    );
    (*env)->ReleaseStringUTFChars(env, cn_asset_root, cn_asset_root_bytes);
    (*env)->ReleaseStringUTFChars(env, root_path, root_path_bytes);
    if (service == NULL) {
        throw_exception(env, "java/lang/IllegalStateException", "Companion personal service failed to start.");
        return 0;
    }
    return (jlong)(uintptr_t)service;
}

JNIEXPORT jint JNICALL Java_dev_starpoint_personalservice_CompanionServiceHost_nativeGetPort(
    JNIEnv *env,
    jclass host_class,
    jlong handle
) {
    (void)env;
    (void)host_class;
    return (jint)starpoint_personal_service_port(service_from_handle(handle));
}

JNIEXPORT jboolean JNICALL Java_dev_starpoint_personalservice_CompanionServiceHost_nativeIsRunning(
    JNIEnv *env,
    jclass host_class,
    jlong handle
) {
    (void)env;
    (void)host_class;
    return starpoint_personal_service_is_running(service_from_handle(handle)) != 0
        ? JNI_TRUE
        : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL Java_dev_starpoint_personalservice_CompanionServiceHost_nativeFlush(
    JNIEnv *env,
    jclass host_class,
    jlong handle
) {
    (void)env;
    (void)host_class;
    return starpoint_personal_service_flush(service_from_handle(handle)) == 0
        ? JNI_TRUE
        : JNI_FALSE;
}

JNIEXPORT void JNICALL Java_dev_starpoint_personalservice_CompanionServiceHost_nativeStop(
    JNIEnv *env,
    jclass host_class,
    jlong handle
) {
    (void)env;
    (void)host_class;
    starpoint_personal_service_stop(service_from_handle(handle));
}
// //// /转换伴随应用个人服务生命周期 ////

// //// 声明 JNI 版本 [@x380kkm 2026-08-31] ////
JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *virtual_machine, void *reserved) {
    (void)virtual_machine;
    (void)reserved;
    return JNI_VERSION_1_6;
}
// //// /声明 JNI 版本 ////
