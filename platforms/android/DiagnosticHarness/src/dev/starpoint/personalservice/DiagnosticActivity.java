// audience: external
// # android-personal-service-diagnostic-activity
//
// 此 Activity 只在前台期间启动同进程个人服务. 生命周期调用在串行工作线程执行,
// WebView 只访问该服务的 loopback 端口.

package dev.starpoint.personalservice;

import android.app.Activity;
import android.net.Uri;
import android.util.Log;
import android.view.ViewGroup;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.TextView;

import java.io.ByteArrayInputStream;
import java.io.File;
import java.util.Collections;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class DiagnosticActivity extends Activity {
    private static final String LOG_TAG = "StarpointPersonalService";
    private static final int PERSONAL_SERVICE_PORT = 17171;
    private static final ExecutorService SERVICE_EXECUTOR = Executors.newSingleThreadExecutor();
    private static long serviceHandle;

    private int lifecycleGeneration;
    private volatile int managementPort;
    private boolean activityDestroyed;
    private WebView managementView;

    // //// 在串行工作线程启动个人服务并显示本机管理页面 [@x380kkm 2026-07-23] ////
    @Override
    protected void onStart() {
        super.onStart();
        int generation = ++lifecycleGeneration;
        SERVICE_EXECUTOR.execute(() -> {
            try {
                ServiceEndpoint endpoint = startPersonalService();
                runOnUiThread(() -> showPersonalService(generation, endpoint));
            } catch (RuntimeException | LinkageError error) {
                stopPersonalService();
                String message = error.getMessage();
                runOnUiThread(() -> showStartFailure(generation, message));
            }
        });
    }

    private ServiceEndpoint startPersonalService() {
        System.loadLibrary("starpoint_android_bridge");
        if (serviceHandle != 0 && !nativeIsRunning(serviceHandle)) {
            nativeStop(serviceHandle);
            serviceHandle = 0;
        }
        if (serviceHandle == 0) {
            File root = new File(getNoBackupFilesDir(), "personal-service");
            if (!root.isDirectory() && !root.mkdirs()) {
                throw new IllegalStateException("无法创建个人服务目录.");
            }
            serviceHandle = nativeStart(root.getAbsolutePath(), PERSONAL_SERVICE_PORT);
        }
        if (serviceHandle == 0) {
            throw new IllegalStateException("个人服务没有返回可用句柄.");
        }
        int port = nativeGetPort(serviceHandle);
        if (port <= 0) {
            throw new IllegalStateException("个人服务没有返回可用端口.");
        }
        String managementToken = nativeCopyManagementToken(serviceHandle);
        if (managementToken == null || managementToken.isEmpty()) {
            throw new IllegalStateException("个人服务没有返回管理 token.");
        }
        return new ServiceEndpoint(port, managementToken);
    }

    private void showPersonalService(int generation, ServiceEndpoint endpoint) {
        if (!isCurrentLifecycle(generation)) {
            return;
        }
        managementPort = endpoint.port;
        if (managementView == null) {
            showManagementPage(endpoint);
        } else {
            loadManagementPage(endpoint);
        }
    }

    private void showManagementPage(ServiceEndpoint endpoint) {
        WebView.setWebContentsDebuggingEnabled(false);
        managementView = new WebView(this);
        WebSettings settings = managementView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(false);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);
        settings.setSaveFormData(false);
        managementView.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                return !isLocalManagementUri(request.getUrl());
            }

            @Override
            public WebResourceResponse shouldInterceptRequest(
                WebView view,
                WebResourceRequest request
            ) {
                if (isLocalManagementUri(request.getUrl())) {
                    return super.shouldInterceptRequest(view, request);
                }
                return createBlockedWebResourceResponse();
            }
        });
        setContentView(
            managementView,
            new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        );
        loadManagementPage(endpoint);
    }

    private void loadManagementPage(ServiceEndpoint endpoint) {
        String url = String.format(
            Locale.ROOT,
            "http://127.0.0.1:%d/manage/#token=%s",
            endpoint.port,
            Uri.encode(endpoint.managementToken)
        );
        managementView.loadUrl(url);
    }

    private boolean isLocalManagementUri(Uri uri) {
        return "http".equals(uri.getScheme())
            && "127.0.0.1".equals(uri.getHost())
            && uri.getPort() == managementPort;
    }

    private static WebResourceResponse createBlockedWebResourceResponse() {
        return new WebResourceResponse(
            "text/plain",
            "UTF-8",
            403,
            "Forbidden",
            Collections.emptyMap(),
            new ByteArrayInputStream(new byte[0])
        );
    }

    private boolean isCurrentLifecycle(int generation) {
        return !activityDestroyed && generation == lifecycleGeneration;
    }

    private void showStartFailure(int generation, String message) {
        if (!isCurrentLifecycle(generation)) {
            return;
        }
        destroyManagementView();
        showFailure(message);
    }

    private void showFailure(String message) {
        TextView failure = new TextView(this);
        failure.setText(message == null ? "个人服务启动失败." : message);
        failure.setPadding(32, 32, 32, 32);
        setContentView(failure);
    }
    // //// /在串行工作线程启动个人服务并显示本机管理页面 ////

    // //// 在 Android 生命周期边界提交并关闭个人服务 [@x380kkm 2026-07-23] ////
    @Override
    protected void onStop() {
        ++lifecycleGeneration;
        managementPort = 0;
        clearManagementPage();
        SERVICE_EXECUTOR.execute(DiagnosticActivity::stopPersonalService);
        super.onStop();
    }

    @Override
    protected void onDestroy() {
        activityDestroyed = true;
        ++lifecycleGeneration;
        managementPort = 0;
        destroyManagementView();
        SERVICE_EXECUTOR.execute(DiagnosticActivity::stopPersonalService);
        super.onDestroy();
    }

    private void clearManagementPage() {
        if (managementView != null) {
            managementView.stopLoading();
            managementView.loadUrl("about:blank");
        }
    }

    private void destroyManagementView() {
        if (managementView != null) {
            clearManagementPage();
            managementView.destroy();
            managementView = null;
        }
    }

    private static void stopPersonalService() {
        if (serviceHandle == 0) {
            return;
        }
        long handle = serviceHandle;
        serviceHandle = 0;
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
    // //// /在 Android 生命周期边界提交并关闭个人服务 ////

    private static final class ServiceEndpoint {
        private final int port;
        private final String managementToken;

        private ServiceEndpoint(int port, String managementToken) {
            this.port = port;
            this.managementToken = managementToken;
        }
    }

    private static native long nativeStart(String rootPath, int requestedPort);
    private static native int nativeGetPort(long handle);
    private static native boolean nativeIsRunning(long handle);
    private static native String nativeCopyManagementToken(long handle);
    private static native boolean nativeFlush(long handle);
    private static native void nativeStop(long handle);
}
