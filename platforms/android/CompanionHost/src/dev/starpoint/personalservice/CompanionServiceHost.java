// audience: internal
// # android-companion-service-host
//
// 该类持有伴随 APK 进程内唯一的个人服务句柄. CDN 从 APK bundle 解包到 externalFilesDir,
// 状态写入 noBackupFilesDir, HTTP 端点固定为 127.0.0.1:17171.
// 状态锁只发布句柄和启动结果. CDN 安装, JNI 调用和状态回调均在状态锁外运行.

package dev.starpoint.personalservice;

import android.content.Context;
import android.util.Log;

import java.io.File;
import java.io.IOException;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.Locale;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;

final class CompanionServiceHost {
    private static final String LOG_TAG = "StarpointPersonalService";
    private static final int PERSONAL_SERVICE_PORT = 17171;

    private static CompanionServiceHost processHost;

    private final Context applicationContext;
    private final Object stateLock = new Object();
    private final Object nativeLock = new Object();
    private long serviceHandle;
    private Endpoint endpoint;
    private CompletableFuture<Endpoint> activeStart;

    private CompanionServiceHost(Context context) {
        applicationContext = context.getApplicationContext();
    }

    static synchronized CompanionServiceHost getOrCreate(Context context) {
        if (processHost == null) {
            processHost = new CompanionServiceHost(context);
        }
        return processHost;
    }

    // //// 用伴随 APK 的持久化目录和外部 CDN 启动固定回环服务 [@x380kkm 2026-09-01] ////
    Endpoint start(CdnAssetInstaller.StatusListener statusListener) {
        CompletableFuture<Endpoint> attempt;
        boolean ownsStart;
        synchronized (stateLock) {
            attempt = activeStart;
            ownsStart = attempt == null;
            if (ownsStart) {
                attempt = new CompletableFuture<>();
                activeStart = attempt;
            }
        }

        if (!ownsStart) {
            return awaitStart(attempt);
        }

        try {
            StartedService service = startOwned(statusListener);
            synchronized (stateLock) {
                serviceHandle = service.handle;
                endpoint = service.endpoint;
            }
            attempt.complete(service.endpoint);
            return service.endpoint;
        } catch (RuntimeException | LinkageError failure) {
            attempt.completeExceptionally(failure);
            throw failure;
        } finally {
            clearActiveStart(attempt);
        }
    }

    private Endpoint awaitStart(CompletableFuture<Endpoint> attempt) {
        try {
            return attempt.get();
        } catch (InterruptedException failure) {
            Thread.currentThread().interrupt();
            throw new IllegalStateException("等待个人服务启动时线程被中断.", failure);
        } catch (ExecutionException failure) {
            Throwable cause = failure.getCause();
            if (cause instanceof RuntimeException) {
                throw (RuntimeException)cause;
            }
            if (cause instanceof LinkageError) {
                throw (LinkageError)cause;
            }
            throw new IllegalStateException("个人服务启动失败.", cause);
        }
    }

    private void clearActiveStart(CompletableFuture<Endpoint> attempt) {
        synchronized (stateLock) {
            if (activeStart == attempt) {
                activeStart = null;
            }
        }
    }

    private StartedService startOwned(CdnAssetInstaller.StatusListener statusListener) {
        System.loadLibrary("starpoint_android_bridge");
        StartedService currentService = probeCurrentService();
        if (currentService != null) {
            return currentService;
        }

        File stateRoot = new File(
            applicationContext.getNoBackupFilesDir(),
            "personal-service/state"
        );
        if (!stateRoot.isDirectory() && !stateRoot.mkdirs()) {
            throw new IllegalStateException("无法创建个人服务状态目录.");
        }
        File cdnRoot = CdnAssetInstaller.requireExternal(applicationContext, statusListener);
        long handle = 0;
        try {
            handle = nativeStart(
                stateRoot.getAbsolutePath(),
                cdnRoot.getAbsolutePath(),
                PERSONAL_SERVICE_PORT
            );
            if (handle == 0) {
                throw new IllegalStateException("个人服务没有返回可用句柄.");
            }

            int port = nativeGetPort(handle);
            if (port != PERSONAL_SERVICE_PORT) {
                throw new IllegalStateException("个人服务没有绑定固定端点.");
            }
            return new StartedService(handle, new Endpoint(port));
        } catch (RuntimeException | LinkageError failure) {
            if (handle != 0) {
                synchronized (nativeLock) {
                    closeNativeHandle(handle, failure);
                }
            }
            throw failure;
        }
    }

    private StartedService probeCurrentService() {
        synchronized (nativeLock) {
            ServiceSnapshot snapshot;
            synchronized (stateLock) {
                snapshot = serviceHandle == 0
                    ? null
                    : new ServiceSnapshot(serviceHandle, endpoint);
            }
            if (snapshot == null) {
                return null;
            }
            boolean running = nativeIsRunning(snapshot.handle);
            int port = running ? nativeGetPort(snapshot.handle) : 0;
            if (running && port == PERSONAL_SERVICE_PORT) {
                Endpoint currentEndpoint = snapshot.endpoint == null
                    ? new Endpoint(port)
                    : snapshot.endpoint;
                return new StartedService(snapshot.handle, currentEndpoint);
            }
            synchronized (stateLock) {
                if (serviceHandle == snapshot.handle) {
                    serviceHandle = 0;
                    endpoint = null;
                }
            }
            closeNativeHandle(snapshot.handle, null);
            return null;
        }
    }
    // //// /用伴随 APK 的持久化目录和外部 CDN 启动固定回环服务 ////

    // //// 查询固定端点并提交个人服务状态 [@x380kkm 2026-09-01] ////
    boolean isRunning() {
        return probeCurrentService() != null;
    }

    boolean isHealthy() {
        StartedService currentService = probeCurrentService();
        if (currentService == null) {
            return false;
        }
        HttpURLConnection connection = null;
        try {
            connection = (HttpURLConnection)new URL(
                String.format(Locale.ROOT, "http://127.0.0.1:%d/health", currentService.endpoint.port)
            ).openConnection();
            connection.setConnectTimeout(1000);
            connection.setReadTimeout(1000);
            connection.setRequestMethod("GET");
            connection.setUseCaches(false);
            return connection.getResponseCode() == HttpURLConnection.HTTP_OK;
        } catch (IOException | RuntimeException failure) {
            return false;
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
        }
    }

    void restart(CdnAssetInstaller.StatusListener statusListener) {
        synchronized (nativeLock) {
            long handle;
            synchronized (stateLock) {
                handle = serviceHandle;
                serviceHandle = 0;
                endpoint = null;
            }
            if (handle != 0) {
                closeNativeHandle(handle, null);
            }
        }
        start(statusListener);
    }

    String managementUrl() {
        StartedService currentService = probeCurrentService();
        if (currentService == null) {
            return null;
        }
        return String.format(
            Locale.ROOT,
            "http://127.0.0.1:%d/manage/",
            currentService.endpoint.port
        );
    }

    boolean flush() {
        synchronized (nativeLock) {
            ServiceSnapshot snapshot;
            synchronized (stateLock) {
                snapshot = serviceHandle == 0
                    ? null
                    : new ServiceSnapshot(serviceHandle, endpoint);
            }
            if (snapshot == null) {
                return true;
            }
            return nativeFlush(snapshot.handle);
        }
    }
    // //// /查询固定端点并提交个人服务状态 ////

    private void closeNativeHandle(long handle, Throwable previousFailure) {
        boolean checkpointCompleted = false;
        Throwable closeFailure = null;
        try {
            checkpointCompleted = nativeFlush(handle);
        } catch (RuntimeException | LinkageError failure) {
            closeFailure = failure;
        }
        try {
            nativeStop(handle);
        } catch (RuntimeException | LinkageError failure) {
            if (closeFailure == null) {
                closeFailure = failure;
            } else {
                closeFailure.addSuppressed(failure);
            }
        }
        if (!checkpointCompleted) {
            Log.e(LOG_TAG, "Personal service checkpoint failed while closing a handle.");
        }
        if (closeFailure == null) {
            return;
        }
        if (previousFailure != null) {
            previousFailure.addSuppressed(closeFailure);
            return;
        }
        if (closeFailure instanceof RuntimeException) {
            throw (RuntimeException)closeFailure;
        }
        throw (LinkageError)closeFailure;
    }
    // //// /关闭失效句柄并提交最后状态 ////

    // //// 保存个人服务启动结果和句柄快照 [@x380kkm 2026-09-01] ////
    static final class Endpoint {
        private final int port;

        private Endpoint(int port) {
            this.port = port;
        }

        int port() {
            return port;
        }
    }

    private static final class StartedService {
        private final long handle;
        private final Endpoint endpoint;

        private StartedService(long handle, Endpoint endpoint) {
            this.handle = handle;
            this.endpoint = endpoint;
        }
    }

    private static final class ServiceSnapshot {
        private final long handle;
        private final Endpoint endpoint;

        private ServiceSnapshot(long handle, Endpoint endpoint) {
            this.handle = handle;
            this.endpoint = endpoint;
        }
    }

    // //// /保存个人服务启动结果和句柄快照 ////

    // //// 声明伴随服务 JNI 边界 [@x380kkm 2026-08-31] ////
    private static native long nativeStart(String rootPath, String cdnRoot, int requestedPort);
    private static native int nativeGetPort(long handle);
    private static native boolean nativeIsRunning(long handle);
    private static native boolean nativeFlush(long handle);
    private static native void nativeStop(long handle);
    // //// /声明伴随服务 JNI 边界 ////
}
