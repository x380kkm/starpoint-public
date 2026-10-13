// audience: external
// # android-personal-service-management-activity
//
// 该 Activity 安装内嵌游戏, 启动持久前台服务并进入游戏或本机管理页面.
// WebView 只访问固定回环端点, Activity 进入后台时伴随服务继续运行.

package dev.starpoint.personalservice;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.ComponentName;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.system.Os;
import android.system.OsConstants;
import android.util.Log;
import android.view.ViewGroup;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.TextView;
import android.widget.Toast;

import java.io.ByteArrayInputStream;
import java.util.Collections;

public final class ManagementActivity extends Activity {
    private static final int MANAGEMENT_PORT = 17171;
    private static final long READY_POLL_MILLISECONDS = 250;
    private static final int NOTIFICATION_PERMISSION_REQUEST = 17171;
    private static final int RESOURCE_DOCUMENT_REQUEST = 17174;
    private static final String DOCUMENT_REQUEST_PENDING = "document_request_pending";
    private static final String GAME_PACKAGE = GameInstaller.GAME_PACKAGE;

    private final Handler mainHandler = new Handler(Looper.getMainLooper());

    private int lifecycleGeneration;
    private boolean activityDestroyed;
    private String displayedStatus;
    private TextView statusView;
    private WebView managementView;
    private ManagementDocuments managementDocuments;
    private boolean launchGameWhenReady;
    private boolean gameLaunchAttempted;
    private boolean installationAttempted;
    private boolean compatibilityConfirmed;
    private boolean compatibilityPromptVisible;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestNotificationPermission();
        launchGameWhenReady = isLauncherIntent(getIntent())
            && (savedInstanceState == null || !savedInstanceState.getBoolean(DOCUMENT_REQUEST_PENDING));
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
        if (showCompatibilityNotice()) {
            return;
        }
        continueStartup(generation);
    }

    private void continueStartup(int generation) {
        if (!isCurrentLifecycle(generation)) {
            return;
        }
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
        if (!GameInstaller.isInstallInProgress(this) && !installationAttempted) {
            installationAttempted = true;
            showStatus(GameInstaller.hasOutdatedInstallation(this)
                ? "正在更新配套游戏..." : "正在准备游戏安装...");
            GameInstaller.beginInstallIfNeeded(this);
        }
        if (GameInstaller.launchPendingUserConfirmation(this)) {
            installationAttempted = true;
            return;
        }
        String status = currentInstallStatus();
        if (GameInstaller.isInstallInProgress(this)) {
            installationAttempted = true;
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
            if (showResourceImportWhenNeeded(generation)) return;
            String failure = PersonalServiceForegroundService.lastStartFailure();
            if (failure != null) {
                showActionStatus(failure + "\n\n点击这里重试启动.", () -> {
                    PersonalServiceForegroundService.ensureStarted(this);
                    launchGameWhenServiceReady(generation);
                });
                return;
            }
            showStatus(CdnAssetInstaller.status());
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
        if (showResourceImportWhenNeeded(generation)) return;
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

    // //// 通过系统文档选择器准备独立资源包 [@x380kkm 2026-09-26] ////
    private boolean showResourceImportWhenNeeded(int generation) {
        if (!CdnAssetInstaller.requiresResourceDocument()) return false;
        if (CdnResourceImport.isRunning()) {
            showStatus(CdnAssetInstaller.status());
        } else {
            String failure = CdnResourceImport.lastFailure();
            String prompt = "请选择与安装包配套的 .starpoint-resources 文件.\n\n点击这里打开系统文件选择器.";
            showActionStatus(failure == null ? prompt : failure + "\n\n" + prompt, this::chooseResourceDocument);
        }
        mainHandler.postDelayed(() -> {
            if (!isCurrentLifecycle(generation)) return;
            if (launchGameWhenReady && !gameLaunchAttempted) launchGameWhenServiceReady(generation);
            else showManagementPageWhenReady(generation);
        }, READY_POLL_MILLISECONDS);
        return true;
    }

    private void chooseResourceDocument() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            startActivityForResult(intent, RESOURCE_DOCUMENT_REQUEST);
        } catch (ActivityNotFoundException | SecurityException error) {
            Toast.makeText(this, "系统文件选择器不可用, 请启用系统文件应用后重试.", Toast.LENGTH_LONG).show();
        }
    }
    // //// /通过系统文档选择器准备独立资源包 ////

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
        ManagementDocuments documents = new ManagementDocuments(this, view);
        managementDocuments = documents;
        view.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageStarted(WebView webView, String url, Bitmap favicon) {
                documents.cancelPageRequests();
            }

            @Override
            public void onPageFinished(WebView webView, String url) {
                if (url.equals(webView.getUrl())) documents.attachToPage();
            }

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

    // //// 显示设备原生库兼容信息 [@x380kkm 2026-09-10] ////
    private boolean showCompatibilityNotice() {
        if (compatibilityConfirmed) {
            return false;
        }
        if (compatibilityPromptVisible) {
            return true;
        }
        long pageSize;
        try {
            pageSize = Os.sysconf(OsConstants._SC_PAGESIZE);
        } catch (RuntimeException error) {
            Log.w("StarpointPersonalService", "Unable to read system page size.", error);
            compatibilityConfirmed = true;
            return false;
        }
        if (pageSize <= 4096) {
            compatibilityConfirmed = true;
            return false;
        }
        compatibilityPromptVisible = true;
        new AlertDialog.Builder(this)
            .setTitle("设备兼容提醒")
            .setMessage("当前系统使用 " + (pageSize / 1024) + " KB 内存页. "
                + "游戏内原生库按 4 KB 构建, 运行依赖系统提供的旧应用兼容模式. "
                + "可在游戏的系统应用详情中查看兼容选项, 或继续尝试运行.")
            .setPositiveButton("继续", (dialog, which) -> {
                compatibilityPromptVisible = false;
                compatibilityConfirmed = true;
                continueStartup(++lifecycleGeneration);
            })
            .setNegativeButton("退出", (dialog, which) -> finish())
            .setNeutralButton("应用详情", (dialog, which) -> {
                compatibilityPromptVisible = false;
                startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                    Uri.parse("package:" + GAME_PACKAGE)));
            })
            .setCancelable(false)
            .show();
        return true;
    }
    // //// /显示设备原生库兼容信息 ////

    // //// 显示服务状态并申请通知权限 [@x380kkm 2026-08-31] ////
    private void showStatus(String message) {
        if (message.equals(displayedStatus) && statusView != null && managementView == null) {
            return;
        }
        displayedStatus = message;
        if (managementView != null) {
            managementDocuments.dispose();
            managementDocuments = null;
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

    // //// 处理界面生命周期边界 [@x380kkm 2026-09-04] ////
    @Override
    protected void onSaveInstanceState(Bundle state) {
        state.putBoolean(DOCUMENT_REQUEST_PENDING,
            managementDocuments != null && managementDocuments.hasPendingRequest());
        super.onSaveInstanceState(state);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == RESOURCE_DOCUMENT_REQUEST && resultCode == RESULT_OK && data != null) {
            CdnResourceImport.start(this, data.getData());
            return;
        }
        if (ManagementDocuments.isDocumentResult(requestCode)) {
            if (managementDocuments != null) {
                managementDocuments.onActivityResult(requestCode, resultCode, data);
            } else if (resultCode == RESULT_OK) {
                Toast.makeText(this, "页面已重新载入, 请重新选择文件或导出存档.", Toast.LENGTH_LONG).show();
            }
        }
    }

    @Override
    protected void onPause() {
        ++lifecycleGeneration;
        mainHandler.removeCallbacksAndMessages(null);
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        activityDestroyed = true;
        ++lifecycleGeneration;
        mainHandler.removeCallbacksAndMessages(null);
        if (managementView != null) {
            managementDocuments.dispose();
            managementDocuments = null;
            managementView.stopLoading();
            managementView.loadUrl("about:blank");
            managementView.destroy();
            managementView = null;
        }
        super.onDestroy();
    }
    // //// /处理界面生命周期边界 ////
}
