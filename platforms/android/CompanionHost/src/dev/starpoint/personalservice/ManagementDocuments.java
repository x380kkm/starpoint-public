// audience: internal
// # android-management-documents
//
// 系统文档选择器授予单个 content URI 的读写权限, 存档内容由本机管理页面提供.
// 导出结果在输出流关闭后回传页面, 页面销毁会撤销尚未交付的文档请求.

package dev.starpoint.personalservice;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Process;
import android.provider.DocumentsContract;
import android.webkit.JavascriptInterface;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebView;
import android.widget.Toast;

import org.json.JSONObject;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.UUID;

public final class ManagementDocuments extends WebChromeClient {
    private static final int OPEN_DOCUMENT = 17172;
    private static final int CREATE_DOCUMENT = 17173;
    private static final String BRIDGE_NAME = "StarpointDocumentBridge";

    private final Activity activity;
    private final WebView webView;
    private ValueCallback<Uri[]> pendingImport;
    private ExportRequest pendingExport;
    private String pageToken;
    private boolean disposed;

    ManagementDocuments(Activity activity, WebView webView) {
        this.activity = activity;
        this.webView = webView;
        webView.setWebChromeClient(this);
        webView.addJavascriptInterface(new ExportBridge(), BRIDGE_NAME);
    }

    // //// 将文件选择限制到用户授权的系统文档 [@x380kkm 2026-09-26] ////
    @Override
    public boolean onShowFileChooser(WebView view, ValueCallback<Uri[]> callback, FileChooserParams params) {
        if (disposed || !isTrustedPage() || params.getMode() != FileChooserParams.MODE_OPEN) {
            callback.onReceiveValue(null);
            return true;
        }
        if (pendingImport != null || pendingExport != null) {
            callback.onReceiveValue(null);
            notifyUser("请先完成当前文件选择.");
            return true;
        }
        pendingImport = callback;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            activity.startActivityForResult(intent, OPEN_DOCUMENT);
        } catch (ActivityNotFoundException | SecurityException error) {
            finishImport(null);
            notifyUser("系统文件选择器不可用, 请启用系统文件应用后重试.");
        }
        return true;
    }

    static boolean isDocumentResult(int requestCode) {
        return requestCode == OPEN_DOCUMENT || requestCode == CREATE_DOCUMENT;
    }

    void onActivityResult(int requestCode, int resultCode, Intent data) {
        Uri uri = resultCode == Activity.RESULT_OK && data != null ? data.getData() : null;
        if (requestCode == OPEN_DOCUMENT) {
            if (pendingImport == null) {
                notifyUser("页面已重新载入, 请重新选择存档文件.");
                return;
            }
            if (uri != null && !isGrantedDocument(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)) {
                notifyUser("文件选择器未授予所选文档的读取权限, 请重新选择.");
                uri = null;
            }
            finishImport(uri);
        } else if (requestCode == CREATE_DOCUMENT) {
            ExportRequest request = pendingExport;
            if (request == null) {
                notifyUser("页面已重新载入, 请重新导出存档.");
                return;
            }
            if (uri == null) {
                finishExport(request, "cancelled", "");
            } else if (!isGrantedDocument(uri, Intent.FLAG_GRANT_WRITE_URI_PERMISSION)) {
                finishExport(request, "error", "文件选择器未授予所选文档的写入权限, 请重新导出.");
            } else {
                writeDocument(request, uri);
            }
        }
    }

    private boolean isGrantedDocument(Uri uri, int permission) {
        return "content".equals(uri.getScheme())
            && DocumentsContract.isDocumentUri(activity, uri)
            && activity.checkUriPermission(uri, Process.myPid(), Process.myUid(), permission)
                == PackageManager.PERMISSION_GRANTED;
    }

    private void finishImport(Uri uri) {
        ValueCallback<Uri[]> callback = pendingImport;
        pendingImport = null;
        if (callback != null) callback.onReceiveValue(uri == null ? null : new Uri[] {uri});
    }
    // //// /将文件选择限制到用户授权的系统文档 ////

    // //// 在可信页面与系统文档写入之间传递导出结果 [@x380kkm 2026-09-26] ////
    void attachToPage() {
        if (disposed || pageToken != null || !isTrustedPage()) return;
        pageToken = UUID.randomUUID().toString();
        String script = "(() => { const token = " + JSONObject.quote(pageToken) + ";"
            + "let sequence = 0; const pending = new Map();"
            + "window.StarpointDocuments = {"
            + "saveJson(filename, contents) { return new Promise((resolve, reject) => {"
            + "const id = ++sequence; pending.set(id, {resolve, reject});"
            + "try { StarpointDocumentBridge.exportJson(token, id, filename, contents); }"
            + "catch (error) { pending.delete(id); reject(error); } }); },"
            + "complete(receivedToken, id, status, message) {"
            + "if (receivedToken !== token) return; const entry = pending.get(id); if (!entry) return;"
            + "pending.delete(id); if (status === 'error') entry.reject(new Error(message));"
            + "else entry.resolve(status); } }; })();";
        webView.evaluateJavascript(script, null);
    }

    public final class ExportBridge {
        @JavascriptInterface
        public void exportJson(String token, int id, String filename, String contents) {
            activity.runOnUiThread(() -> beginExport(token, id, filename, contents));
        }
    }

    private void beginExport(String token, int id, String filename, String contents) {
        if (disposed || pageToken == null || !pageToken.equals(token) || !isTrustedPage()) return;
        ExportRequest request = new ExportRequest(token, id, contents);
        if (pendingImport != null || pendingExport != null) {
            sendResult(request, "error", "请先完成当前文件选择.");
            return;
        }
        if (filename == null || contents == null || filename.isEmpty()
                || filename.indexOf('/') >= 0 || filename.indexOf('\\') >= 0
                || filename.chars().anyMatch(character -> character < 32)) {
            sendResult(request, "error", "导出文件名或内容无效.");
            return;
        }
        pendingExport = request;
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(filename.endsWith(".json") ? "application/json" : "application/octet-stream");
        intent.putExtra(Intent.EXTRA_TITLE, filename);
        intent.addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        try {
            activity.startActivityForResult(intent, CREATE_DOCUMENT);
        } catch (ActivityNotFoundException | SecurityException error) {
            finishExport(request, "error", "系统文件选择器不可用, 请启用系统文件应用后重试.");
        }
    }

    private void writeDocument(ExportRequest request, Uri uri) {
        new Thread(() -> {
            String failure = null;
            try (OutputStream output = activity.getContentResolver().openOutputStream(uri, "wt")) {
                if (output == null) throw new IOException("Document provider returned no output stream.");
                output.write(request.contents.getBytes(StandardCharsets.UTF_8));
                output.flush();
            } catch (IOException | RuntimeException error) {
                failure = "存档文件写入失败, 所选文件可能不完整. 请重新导出.";
            }
            String resultFailure = failure;
            activity.runOnUiThread(() -> finishExport(request,
                resultFailure == null ? "saved" : "error", resultFailure == null ? "" : resultFailure));
        }, "starpoint-document-export").start();
    }

    private void finishExport(ExportRequest request, String status, String message) {
        if (pendingExport == request) pendingExport = null;
        sendResult(request, status, message);
    }

    private void sendResult(ExportRequest request, String status, String message) {
        if (disposed || !request.token.equals(pageToken) || !isTrustedPage()) return;
        webView.evaluateJavascript("window.StarpointDocuments && window.StarpointDocuments.complete("
            + JSONObject.quote(request.token) + "," + request.id + "," + JSONObject.quote(status)
            + "," + JSONObject.quote(message) + ");", null);
    }
    // //// /在可信页面与系统文档写入之间传递导出结果 ////

    // //// 隔离页面切换与 Activity 回收后的文档请求 [@x380kkm 2026-09-26] ////
    void cancelPageRequests() {
        finishImport(null);
        if (pendingExport != null) finishExport(pendingExport, "cancelled", "");
        pageToken = null;
    }

    void dispose() {
        cancelPageRequests();
        disposed = true;
        webView.removeJavascriptInterface(BRIDGE_NAME);
    }

    boolean hasPendingRequest() {
        return pendingImport != null || pendingExport != null;
    }

    private boolean isTrustedPage() {
        String url = webView.getUrl();
        if (url == null) return false;
        Uri uri = Uri.parse(url);
        String path = uri.getPath();
        return "http".equals(uri.getScheme()) && "127.0.0.1".equals(uri.getHost())
            && uri.getPort() == 17171
            && ("/manage".equals(path) || "/manage/".equals(path)
                || "/player".equals(path) || "/player/".equals(path));
    }

    private void notifyUser(String message) {
        if (!disposed) Toast.makeText(activity, message, Toast.LENGTH_LONG).show();
    }

    private static final class ExportRequest {
        final String token;
        final int id;
        final String contents;

        ExportRequest(String token, int id, String contents) {
            this.token = token;
            this.id = id;
            this.contents = contents;
        }
    }
    // //// /隔离页面切换与 Activity 回收后的文档请求 ////
}
