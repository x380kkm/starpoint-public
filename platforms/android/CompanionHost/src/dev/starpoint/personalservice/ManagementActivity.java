// audience: external
// # android-personal-service-management-activity
//
// 该 Activity 安装内嵌游戏, 启动持久前台服务并进入游戏或本机管理页面.
// WebView 只访问固定回环端点, Activity 进入后台时提交状态, 伴随服务继续运行.

package dev.starpoint.personalservice;

import android.Manifest;
import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.view.ViewGroup;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.TextView;

import java.io.ByteArrayInputStream;
import java.util.Collections;

public final class ManagementActivity extends Activity {
    private static final int MANAGEMENT_PORT = 17171;
    private static final long READY_POLL_MILLISECONDS = 250;
    private static final int NOTIFICATION_PERMISSION_REQUEST = 17171;
    private static final String GAME_PACKAGE = GameInstaller.GAME_PACKAGE;

    private final Handler mainHandler = new Handler(Looper.getMainLooper());

    private int lifecycleGeneration;
    private boolean activityDestroyed;
    private String displayedStatus;
    private TextView statusView;
    private WebView managementView;
    private boolean launchGameWhenReady;
    private boolean gameLaunchAttempted;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestNotificationPermission();
        launchGameWhenReady = isLauncherIntent(getIntent());
        handleInstallIntent(getIntent());
        if (GameInstaller.isInstalled(this)) {
            showStatus("正在启动个人服务...");
        } else {
            showStatus("正在准备游戏安装...");
        }
    }

    // //// 安装游戏并在个人服务就绪后进入游戏或管理页 [@x380kkm 2026-08-31] ////
    @Override
    protected void onResume() {
        super.onResume();
        int generation = ++lifecycleGeneration;
        if (!GameInstaller.isInstalled(this)) {
            showGameInstallWhenReady(generation);
            return;
        }
        PersonalServiceForegroundService.ensureStarted(this);
        if (launchGameWhenReady && !gameLaunchAttempted) {
            launchGameWhenServiceReady(generation);
            return;
        }
        showManagementPageWhenReady(generation);
    }

    private void showGameInstallWhenReady(int generation) {
        if (!isCurrentLifecycle(generation)) {
            return;
        }
        if (GameInstaller.isInstalled(this)) {
            PersonalServiceForegroundService.ensureStarted(this);
            launchGameWhenReady = true;
            launchGameWhenServiceReady(generation);
            return;
        }
        if (GameInstaller.hasIncompatibleInstallation(this)) {
            showStatus(
                "检测到现有游戏签名与此分发包不一致. "
                    + "请先在系统设置中卸载现有游戏, 再返回 Starpoint 重试."
            );
            return;
        }
        if (GameInstaller.hasUnverifiableInstallation(this)) {
            showStatus("无法验证现有游戏签名. 请重新打开 Starpoint 后重试.");
            return;
        }
        if (!canRequestPackageInstalls()) {
            showActionStatus(
                "需要允许 Starpoint 安装游戏. 点击这里打开系统设置.",
                this::openInstallPermissionSettings
            );
            return;
        }
        String persistedStatus = GameInstaller.status(this);
        if (!GameInstaller.isInstallInProgress(this) && persistedStatus == null) {
            GameInstaller.beginInstallIfNeeded(this);
        }
        String status = currentInstallStatus();
        if (GameInstaller.isInstallInProgress(this)) {
            showStatus(status);
            mainHandler.postDelayed(
                () -> showGameInstallWhenReady(generation),
                READY_POLL_MILLISECONDS
            );
            return;
        }
        showActionStatus(status + "\n\n点击这里重试.", () -> {
            GameInstaller.beginInstallIfNeeded(this);
            showGameInstallWhenReady(generation);
        });
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (isLauncherIntent(intent)) {
            launchGameWhenReady = true;
            gameLaunchAttempted = false;
        }
        handleInstallIntent(intent);
    }

    private void handleInstallIntent(Intent intent) {
        if (intent == null) {
            return;
        }
        if (intent.getBooleanExtra(GameInstaller.EXTRA_INSTALL_SUCCEEDED, false)) {
            launchGameWhenReady = true;
            gameLaunchAttempted = false;
            GameInstaller.setStatus(this, "游戏安装完成, 正在启动个人服务...");
        }
        String failure = intent.getStringExtra(GameInstaller.EXTRA_INSTALL_FAILURE);
        if (failure != null) {
            launchGameWhenReady = false;
            gameLaunchAttempted = false;
            showStatus("游戏安装失败: " + failure);
        }
    }

    private String currentInstallStatus() {
        String status = GameInstaller.status(this);
        return status == null ? "正在准备游戏安装..." : status;
    }

    private boolean canRequestPackageInstalls() {
        return Build.VERSION.SDK_INT < 26
            || getPackageManager().canRequestPackageInstalls();
    }

    private void openInstallPermissionSettings() {
        Intent settings = new Intent(
            Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
            Uri.parse("package:" + getPackageName())
        );
        try {
            startActivity(settings);
        } catch (RuntimeException error) {
            startActivity(new Intent(Settings.ACTION_SECURITY_SETTINGS));
        }
    }

    private static boolean isLauncherIntent(Intent intent) {
        return intent != null
            && Intent.ACTION_MAIN.equals(intent.getAction())
            && intent.hasCategory(Intent.CATEGORY_LAUNCHER);
    }

    private void launchGameWhenServiceReady(int generation) {
        if (!isCurrentLifecycle(generation)) {
            return;
        }
        if (CompanionServiceHost.getOrCreate(this).managementUrl() == null) {
            showStatus("正在启动个人服务...");
            mainHandler.postDelayed(
                () -> launchGameWhenServiceReady(generation),
                READY_POLL_MILLISECONDS
            );
            return;
        }
        gameLaunchAttempted = true;
        if (!launchGame()) {
            launchGameWhenReady = false;
            showStatus("游戏已安装, 但没有找到可启动的游戏入口.");
            showManagementPageWhenReady(generation);
        }
    }

    private boolean launchGame() {
        Intent launchIntent = getPackageManager().getLaunchIntentForPackage(GAME_PACKAGE);
        if (launchIntent == null) {
            ComponentName component = new ComponentName(
                GAME_PACKAGE,
                "air.com.leiting.wf.AppEntry"
            );
            launchIntent = new Intent(Intent.ACTION_MAIN).setComponent(component);
        }
        if (launchIntent.resolveActivity(getPackageManager()) == null) {
            return false;
        }
        launchIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            startActivity(launchIntent);
            launchGameWhenReady = false;
            return true;
        } catch (RuntimeException error) {
            return false;
        }
    }

    private void showManagementPageWhenReady(int generation) {
        if (!isCurrentLifecycle(generation)) {
            return;
        }
        String managementUrl = CompanionServiceHost.getOrCreate(this).managementUrl();
        if (managementUrl != null) {
            showManagementPage(managementUrl);
            return;
        }
        String failure = PersonalServiceForegroundService.lastStartFailure();
        showStatus(failure == null ? CdnAssetInstaller.status() : failure);
        mainHandler.postDelayed(
            () -> showManagementPageWhenReady(generation),
            READY_POLL_MILLISECONDS
        );
    }

    private void showManagementPage(String managementUrl) {
        if (managementView == null) {
            managementView = createManagementView();
            setContentView(
                managementView,
                new ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT
                )
            );
        }
        if (!managementUrl.equals(managementView.getUrl())) {
            managementView.loadUrl(managementUrl);
        }
    }
    // //// /安装游戏并在个人服务就绪后进入游戏或管理页 ////

    // //// 限制管理 WebView 只访问固定回环服务 [@x380kkm 2026-08-31] ////
    private WebView createManagementView() {
        WebView.setWebContentsDebuggingEnabled(false);
        WebView view = new WebView(this);
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(false);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);
        settings.setSaveFormData(false);
        view.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                return !isLocalManagementUri(request.getUrl());
            }

            @Override
            public WebResourceResponse shouldInterceptRequest(
                WebView webView,
                WebResourceRequest request
            ) {
                if (isLocalManagementUri(request.getUrl())) {
                    return super.shouldInterceptRequest(webView, request);
                }
                return createBlockedWebResourceResponse();
            }
        });
        return view;
    }

    private static boolean isLocalManagementUri(Uri uri) {
        return "http".equals(uri.getScheme())
            && "127.0.0.1".equals(uri.getHost())
            && uri.getPort() == MANAGEMENT_PORT;
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
    // //// /限制管理 WebView 只访问固定回环服务 ////

    // //// 显示服务状态并申请通知权限 [@x380kkm 2026-08-31] ////
    private void showStatus(String message) {
        if (message.equals(displayedStatus) && statusView != null && managementView == null) {
            return;
        }
        displayedStatus = message;
        if (managementView != null) {
            managementView.stopLoading();
            managementView.destroy();
            managementView = null;
        }
        if (statusView == null) {
            statusView = new TextView(this);
            statusView.setPadding(32, 32, 32, 32);
            statusView.setTextSize(18);
        }
        statusView.setOnClickListener(null);
        statusView.setClickable(false);
        statusView.setText(message);
        setContentView(statusView);
    }

    private void showActionStatus(String message, Runnable action) {
        showStatus(message);
        statusView.setClickable(true);
        statusView.setOnClickListener(view -> action.run());
    }

    private void requestNotificationPermission() {
        if (
            Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                    != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(
                new String[] {Manifest.permission.POST_NOTIFICATIONS},
                NOTIFICATION_PERMISSION_REQUEST
            );
        }
    }

    private boolean isCurrentLifecycle(int generation) {
        return !activityDestroyed && generation == lifecycleGeneration;
    }
    // //// /显示服务状态并申请通知权限 ////

    // //// 在界面生命周期边界提交状态并释放 WebView [@x380kkm 2026-08-31] ////
    @Override
    protected void onPause() {
        ++lifecycleGeneration;
        mainHandler.removeCallbacksAndMessages(null);
        if (CompanionServiceHost.getOrCreate(this).isRunning()) {
            PersonalServiceForegroundService.requestFlush(this);
        }
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        activityDestroyed = true;
        ++lifecycleGeneration;
        mainHandler.removeCallbacksAndMessages(null);
        if (managementView != null) {
            managementView.stopLoading();
            managementView.loadUrl("about:blank");
            managementView.destroy();
            managementView = null;
        }
        super.onDestroy();
    }
    // //// /在界面生命周期边界提交状态并释放 WebView ////
}
