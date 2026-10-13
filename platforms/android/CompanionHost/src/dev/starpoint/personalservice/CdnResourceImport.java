// audience: internal
// # android-cdn-resource-import
//
// 资源导入只读取系统文档选择器授权的 content URI, 运行状态独立于 Activity 生命周期.
// 导入线程复用 CDN 安装器的清单验证与目录提交, 输入流随本次导入关闭.

package dev.starpoint.personalservice;

import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Process;
import android.provider.DocumentsContract;

import java.io.IOException;
import java.io.InputStream;

final class CdnResourceImport {
    private static volatile boolean running;
    private static volatile String failure;

    private CdnResourceImport() {}

    static boolean isRunning() { return running; }
    static String lastFailure() { return failure; }

    // //// 持有用户选择的资源文档并在后台完成安装 [@x380kkm 2026-09-26] ////
    static synchronized void start(Context context, Uri uri) {
        if (running) return;
        Context application = context.getApplicationContext();
        if (uri == null || !"content".equals(uri.getScheme())
                || !DocumentsContract.isDocumentUri(application, uri)
                || application.checkUriPermission(uri, Process.myPid(), Process.myUid(),
                    Intent.FLAG_GRANT_READ_URI_PERMISSION) != PackageManager.PERMISSION_GRANTED) {
            failure = "文件选择器未授予资源文档的读取权限, 请重新选择.";
            return;
        }
        failure = null;
        running = true;
        new Thread(() -> importDocument(application, uri), "starpoint-resource-import").start();
    }

    private static void importDocument(Context context, Uri uri) {
        try {
            try (InputStream source = context.getContentResolver().openInputStream(uri)) {
                if (source == null) throw new IOException("无法读取资源文档.");
                CdnAssetInstaller.importResourceDocument(context, source, null);
            }
            PersonalServiceForegroundService.ensureStarted(context);
        } catch (IOException | RuntimeException error) {
            String detail = error.getMessage();
            failure = "资源导入失败" + (detail == null ? "." : ": " + detail);
        } finally {
            running = false;
        }
    }
    // //// /持有用户选择的资源文档并在后台完成安装 ////
}
