// audience: external
// # android-personal-service-bootstrap
//
// 该类为正式游戏 Activity 提供可嵌入的个人服务生命周期. 调用方在后台线程调用 start,
// 在 Activity 暂停前调用 flush, 并在宿主销毁时调用 stop.

package dev.starpoint.personalservice;

import android.content.Context;
import android.net.Uri;
import android.util.Log;

import java.io.File;
import java.util.Locale;

public final class PersonalServiceBootstrap {
    private static final String LOG_TAG = "StarpointPersonalService";
    private static final int DEFAULT_PORT = 17171;

    private final Context applicationContext;
    private long serviceHandle;
    private Endpoint endpoint;

    public PersonalServiceBootstrap(Context context) {
        if (context == null) {
            throw new NullPointerException("context");
        }
        applicationContext = context.getApplicationContext();
    }

    // //// 在当前 App 数据目录启动个人服务 [@x380kkm 2026-07-24] ////
    public synchronized Endpoint start() {
        System.loadLibrary("starpoint_android_bridge");
        if (serviceHandle != 0 && nativeIsRunning(serviceHandle)) {
            return endpoint;
        }
        if (serviceHandle != 0) {
            nativeFlush(serviceHandle);
            nativeStop(serviceHandle);
            serviceHandle = 0;
            endpoint = null;
        }
        File root = new File(applicationContext.getNoBackupFilesDir(), "personal-service");
        if (!root.isDirectory() && !root.mkdirs()) {
            throw new IllegalStateException("无法创建个人服务目录.");
        }
        serviceHandle = nativeStart(root.getAbsolutePath(), DEFAULT_PORT);
        if (serviceHandle == 0) {
            throw new IllegalStateException("个人服务没有返回可用句柄.");
        }
        int port = nativeGetPort(serviceHandle);
        String token = nativeCopyManagementToken(serviceHandle);
        if (port <= 0 || token == null || token.isEmpty()) {
            nativeStop(serviceHandle);
            serviceHandle = 0;
            throw new IllegalStateException("个人服务没有返回可用端点.");
        }
        endpoint = new Endpoint(port, token);
        return endpoint;
    }
    // //// /在当前 App 数据目录启动个人服务 ////

    public synchronized boolean isRunning() {
        return serviceHandle != 0 && nativeIsRunning(serviceHandle);
    }

    // //// 在 App 暂停前提交个人服务持久化状态 [@x380kkm 2026-07-24] ////
    public synchronized boolean flush() {
        return serviceHandle == 0 || nativeFlush(serviceHandle);
    }
    // //// /在 App 暂停前提交个人服务持久化状态 ////

    // //// 在宿主销毁时提交并关闭个人服务 [@x380kkm 2026-07-24] ////
    public synchronized void stop() {
        if (serviceHandle == 0) {
            return;
        }
        long handle = serviceHandle;
        serviceHandle = 0;
        endpoint = null;
        boolean checkpointCompleted;
        try {
            checkpointCompleted = nativeFlush(handle);
        } finally {
            nativeStop(handle);
        }
        if (!checkpointCompleted) {
            Log.e(LOG_TAG, "Personal service checkpoint failed before stop.");
        }
    }
    // //// /在宿主销毁时提交并关闭个人服务 ////

    public synchronized Endpoint endpoint() {
        return endpoint;
    }

    public synchronized String managementUrl() {
        if (endpoint == null) {
            return null;
        }
        return String.format(
            Locale.ROOT,
            "http://127.0.0.1:%d/manage/#token=%s",
            endpoint.port(),
            Uri.encode(endpoint.managementToken())
        );
    }

    public static final class Endpoint {
        private final int port;
        private final String managementToken;

        private Endpoint(int port, String managementToken) {
            this.port = port;
            this.managementToken = managementToken;
        }

        public int port() {
            return port;
        }

        public String managementToken() {
            return managementToken;
        }
    }

    private static native long nativeStart(String rootPath, int requestedPort);
    private static native int nativeGetPort(long handle);
    private static native boolean nativeIsRunning(long handle);
    private static native String nativeCopyManagementToken(long handle);
    private static native boolean nativeFlush(long handle);
    private static native void nativeStop(long handle);
}
