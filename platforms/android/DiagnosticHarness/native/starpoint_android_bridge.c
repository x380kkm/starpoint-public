// audience: external
// # android-personal-service-jni
//
// 该文件把 Android JNI 调用转换为个人服务 C ABI. Java 层拥有并串行释放句柄.

#include <jni.h>
#include <stdint.h>
#include <stdlib.h>

#include "starpoint_personal_service.h"

static StarpointPersonalService *service_from_handle(jlong handle) {
    return (StarpointPersonalService *)(uintptr_t)handle;
}

static void throw_exception(JNIEnv *env, const char *class_name, const char *message) {
    jclass exception_class = (*env)->FindClass(env, class_name);
    if (exception_class != NULL) {
        (*env)->ThrowNew(env, exception_class, message);
    }
}

// //// 转换个人服务生命周期和管理 token [@x380kkm 2026-07-23] ////
JNIEXPORT jlong JNICALL
Java_dev_starpoint_personalservice_DiagnosticActivity_nativeStart(
    JNIEnv *env,
    jclass activity_class,
    jstring root_path,
    jint requested_port
) {
    (void)activity_class;
    if (root_path == NULL || requested_port < 0 || requested_port > UINT16_MAX) {
        throw_exception(env, "java/lang/IllegalArgumentException", "Invalid personal service start arguments.");
        return 0;
    }
    const char *root_path_bytes = (*env)->GetStringUTFChars(env, root_path, NULL);
    if (root_path_bytes == NULL) {
        return 0;
    }
    StarpointPersonalService *service = starpoint_personal_service_start(
        root_path_bytes,
        (uint16_t)requested_port
    );
    (*env)->ReleaseStringUTFChars(env, root_path, root_path_bytes);
    if (service == NULL) {
        throw_exception(env, "java/lang/IllegalStateException", "Personal service failed to start.");
        return 0;
    }
    return (jlong)(uintptr_t)service;
}

JNIEXPORT jint JNICALL
Java_dev_starpoint_personalservice_DiagnosticActivity_nativeGetPort(
    JNIEnv *env,
    jclass activity_class,
    jlong handle
) {
    (void)env;
    (void)activity_class;
    return (jint)starpoint_personal_service_port(service_from_handle(handle));
}

JNIEXPORT jboolean JNICALL
Java_dev_starpoint_personalservice_DiagnosticActivity_nativeIsRunning(
    JNIEnv *env,
    jclass activity_class,
    jlong handle
) {
    (void)env;
    (void)activity_class;
    return starpoint_personal_service_is_running(service_from_handle(handle)) != 0
        ? JNI_TRUE
        : JNI_FALSE;
}

JNIEXPORT jstring JNICALL
Java_dev_starpoint_personalservice_DiagnosticActivity_nativeCopyManagementToken(
    JNIEnv *env,
    jclass activity_class,
    jlong handle
) {
    (void)activity_class;
    StarpointPersonalService *service = service_from_handle(handle);
    size_t required = starpoint_personal_service_copy_management_token(service, NULL, 0);
    if (required == 0) {
        return NULL;
    }
    char *token = (char *)malloc(required);
    if (token == NULL) {
        throw_exception(env, "java/lang/OutOfMemoryError", "Management token allocation failed.");
        return NULL;
    }
    size_t copied = starpoint_personal_service_copy_management_token(service, token, required);
    if (copied != required) {
        free(token);
        throw_exception(env, "java/lang/IllegalStateException", "Management token copy failed.");
        return NULL;
    }
    jstring result = (*env)->NewStringUTF(env, token);
    free(token);
    return result;
}

JNIEXPORT jboolean JNICALL
Java_dev_starpoint_personalservice_DiagnosticActivity_nativeFlush(
    JNIEnv *env,
    jclass activity_class,
    jlong handle
) {
    (void)env;
    (void)activity_class;
    return starpoint_personal_service_flush(service_from_handle(handle)) == 0
        ? JNI_TRUE
        : JNI_FALSE;
}

JNIEXPORT void JNICALL
Java_dev_starpoint_personalservice_DiagnosticActivity_nativeStop(
    JNIEnv *env,
    jclass activity_class,
    jlong handle
) {
    (void)env;
    (void)activity_class;
    starpoint_personal_service_stop(service_from_handle(handle));
}
// //// /转换个人服务生命周期和管理 token ////

// //// 为正式游戏宿主复用个人服务 JNI 契约 [@x380kkm 2026-07-24] ////
JNIEXPORT jlong JNICALL
Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeStart(
    JNIEnv *env,
    jclass bootstrap_class,
    jstring root_path,
    jint requested_port
) {
    return Java_dev_starpoint_personalservice_DiagnosticActivity_nativeStart(
        env,
        bootstrap_class,
        root_path,
        requested_port
    );
}

JNIEXPORT jint JNICALL
Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeGetPort(
    JNIEnv *env,
    jclass bootstrap_class,
    jlong handle
) {
    return Java_dev_starpoint_personalservice_DiagnosticActivity_nativeGetPort(
        env,
        bootstrap_class,
        handle
    );
}

JNIEXPORT jboolean JNICALL
Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeIsRunning(
    JNIEnv *env,
    jclass bootstrap_class,
    jlong handle
) {
    return Java_dev_starpoint_personalservice_DiagnosticActivity_nativeIsRunning(
        env,
        bootstrap_class,
        handle
    );
}

JNIEXPORT jstring JNICALL
Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeCopyManagementToken(
    JNIEnv *env,
    jclass bootstrap_class,
    jlong handle
) {
    return Java_dev_starpoint_personalservice_DiagnosticActivity_nativeCopyManagementToken(
        env,
        bootstrap_class,
        handle
    );
}

JNIEXPORT jboolean JNICALL
Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeFlush(
    JNIEnv *env,
    jclass bootstrap_class,
    jlong handle
) {
    return Java_dev_starpoint_personalservice_DiagnosticActivity_nativeFlush(
        env,
        bootstrap_class,
        handle
    );
}

JNIEXPORT void JNICALL
Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeStop(
    JNIEnv *env,
    jclass bootstrap_class,
    jlong handle
) {
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeStop(
        env,
        bootstrap_class,
        handle
    );
}
// //// /为正式游戏宿主复用个人服务 JNI 契约 ////

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *virtual_machine, void *reserved) {
    (void)virtual_machine;
    (void)reserved;
    return JNI_VERSION_1_6;
}
