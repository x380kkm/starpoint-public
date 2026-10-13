// audience: internal
// # android-game-installer
//
// 该类把伴随 APK 内的游戏包提交给 Android 系统安装器. 安装动作由系统页面交给用户确认,
// 同签名游戏按内嵌版本覆盖安装, 安装会话在进程重启后恢复或撤销.

package dev.starpoint.personalservice;

import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.IntentSender;
import android.content.pm.PackageInfo;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.content.pm.SigningInfo;
import android.content.res.AssetFileDescriptor;
import android.os.Build;
import android.util.Log;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Collections;
import java.util.HashSet;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

final class GameInstaller {
    static final String GAME_PACKAGE = "com.leiting.wf";
    static final String GAME_ASSET = "starpoint-game.apk";
    static final String GAME_SIGNERS_ASSET = "starpoint-game-signers.sha256";
    static final String GAME_VERSION_ASSET = "starpoint-game-version.txt";
    static final String INSTALL_RESULT_ACTION =
        "dev.starpoint.personalservice.action.GAME_INSTALL_RESULT";
    static final String EXTRA_INSTALL_SUCCEEDED =
        "dev.starpoint.personalservice.extra.GAME_INSTALL_SUCCEEDED";
    static final String EXTRA_INSTALL_FAILURE =
        "dev.starpoint.personalservice.extra.GAME_INSTALL_FAILURE";

    private static final String PREFERENCES = "game-installer";
    private static final String STATUS_KEY = "status";
    private static final String SESSION_KEY = "session-id";
    private static final String SESSION_VERSION_KEY = "session-version";
    private static final int COPY_BUFFER_SIZE = 64 * 1024;

    private static final ExecutorService INSTALL_EXECUTOR =
        Executors.newSingleThreadExecutor();
    private static boolean installScheduled;
    private static Set<String> expectedSigners;
    private static Long expectedVersionCode;
    private static int submittedSessionId = -1;
    private static Intent pendingConfirmation;
    private static boolean confirmationLaunched;

    private GameInstaller() {
    }

    // //// 查询游戏签名和可用版本 [@x380kkm 2026-09-10] ////
    static boolean isInstalled(Context context) {
        return installedState(context) == InstalledState.COMPATIBLE;
    }

    static boolean hasIncompatibleInstallation(Context context) {
        return installedState(context) == InstalledState.SIGNATURE_MISMATCH;
    }

    static boolean hasUnverifiableInstallation(Context context) {
        return installedState(context) == InstalledState.UNVERIFIABLE;
    }

    static boolean hasOutdatedInstallation(Context context) {
        return installedState(context) == InstalledState.UPDATE_REQUIRED;
    }

    // //// 验证游戏 Binder 调用者的 UID 和签名 [@x380kkm 2026-09-10] ////
    static boolean isTrustedGameCaller(Context context, int callingUid) {
        try {
            PackageManager manager = context.getPackageManager();
            String[] packages = manager.getPackagesForUid(callingUid);
            if (packages == null || !java.util.Arrays.asList(packages).contains(GAME_PACKAGE)) {
                return false;
            }
            PackageInfo info = installedPackageInfo(manager);
            return info.applicationInfo != null
                && info.applicationInfo.uid == callingUid
                && !Collections.disjoint(
                    installedSignerDigests(info),
                    expectedSignerDigests(context)
                );
        } catch (IOException | PackageManager.NameNotFoundException | RuntimeException error) {
            return false;
        }
    }
    // //// /验证游戏 Binder 调用者的 UID 和签名 ////

    private static InstalledState installedState(Context context) {
        PackageInfo packageInfo;
        try {
            packageInfo = installedPackageInfo(context.getPackageManager());
        } catch (PackageManager.NameNotFoundException ignored) {
            return InstalledState.ABSENT;
        } catch (RuntimeException error) {
            return InstalledState.UNVERIFIABLE;
        }
        try {
            Set<String> installedSigners = installedSignerDigests(packageInfo);
            Set<String> requiredSigners = expectedSignerDigests(context);
            if (Collections.disjoint(installedSigners, requiredSigners)) {
                return InstalledState.SIGNATURE_MISMATCH;
            }
            long versionCode = Build.VERSION.SDK_INT >= 28
                ? packageInfo.getLongVersionCode()
                : packageInfo.versionCode;
            return versionCode >= expectedGameVersionCode(context)
                ? InstalledState.COMPATIBLE
                : InstalledState.UPDATE_REQUIRED;
        } catch (IOException | RuntimeException error) {
            return InstalledState.UNVERIFIABLE;
        }
    }

    private static PackageInfo installedPackageInfo(PackageManager packageManager)
        throws PackageManager.NameNotFoundException {
        if (Build.VERSION.SDK_INT >= 28) {
            return packageManager.getPackageInfo(
                GAME_PACKAGE,
                PackageManager.GET_SIGNING_CERTIFICATES
            );
        }
        return packageManager.getPackageInfo(GAME_PACKAGE, PackageManager.GET_SIGNATURES);
    }

    private static Set<String> installedSignerDigests(PackageInfo packageInfo) {
        Signature[] signatures;
        if (Build.VERSION.SDK_INT >= 28) {
            SigningInfo signingInfo = packageInfo.signingInfo;
            if (signingInfo == null) {
                throw new IllegalStateException("已安装游戏没有签名信息.");
            }
            signatures = signingInfo.hasMultipleSigners()
                ? signingInfo.getApkContentsSigners()
                : signingInfo.getSigningCertificateHistory();
        } else {
            signatures = packageInfo.signatures;
        }
        if (signatures == null || signatures.length == 0) {
            throw new IllegalStateException("已安装游戏没有签名证书.");
        }
        Set<String> digests = new HashSet<>();
        for (Signature signature : signatures) {
            digests.add(sha256(signature.toByteArray()));
        }
        return digests;
    }

    private static synchronized Set<String> expectedSignerDigests(Context context)
        throws IOException {
        if (expectedSigners != null) {
            return expectedSigners;
        }
        Set<String> signers = new HashSet<>();
        try (
            BufferedReader reader = new BufferedReader(
                new InputStreamReader(
                    context.getAssets().open(GAME_SIGNERS_ASSET),
                    StandardCharsets.UTF_8
                )
            )
        ) {
            String line;
            while ((line = reader.readLine()) != null) {
                String digest = line.trim().toLowerCase(Locale.ROOT);
                if (digest.isEmpty()) {
                    continue;
                }
                if (!digest.matches("[0-9a-f]{64}")) {
                    throw new IOException("内嵌游戏签名摘要格式无效.");
                }
                signers.add(digest);
            }
        }
        if (signers.isEmpty()) {
            throw new IOException("内嵌游戏签名摘要为空.");
        }
        expectedSigners = Collections.unmodifiableSet(signers);
        return expectedSigners;
    }

    private static synchronized long expectedGameVersionCode(Context context) throws IOException {
        if (expectedVersionCode != null) {
            return expectedVersionCode;
        }
        try (
            BufferedReader reader = new BufferedReader(new InputStreamReader(
                context.getAssets().open(GAME_VERSION_ASSET),
                StandardCharsets.UTF_8
            ))
        ) {
            String value = reader.readLine();
            if (value == null || !value.matches("[0-9]+") || reader.readLine() != null) {
                throw new IOException("内嵌游戏版本号格式无效.");
            }
            long version = Long.parseLong(value);
            if (version < 1_008_001L) {
                throw new IOException("内嵌游戏版本号过低.");
            }
            expectedVersionCode = version;
            return version;
        } catch (NumberFormatException error) {
            throw new IOException("内嵌游戏版本号超出范围.", error);
        }
    }
    // //// /查询游戏签名和可用版本 ////

    // //// 保存安装状态并转交用户确认 [@x380kkm 2026-09-10] ////
    static synchronized String status(Context context) {
        return context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(STATUS_KEY, null);
    }

    static synchronized void setStatus(Context context, String status) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(STATUS_KEY, status)
            .apply();
    }

    static synchronized void clearSession(Context context) {
        submittedSessionId = -1;
        pendingConfirmation = null;
        confirmationLaunched = false;
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .remove(SESSION_KEY)
            .remove(SESSION_VERSION_KEY)
            .apply();
    }

    static synchronized boolean isCurrentSession(Context context, int sessionId) {
        return sessionId >= 0 && sessionId == storedSessionId(context);
    }

    static synchronized void cancelSession(Context context, int sessionId, String message) {
        if (!isCurrentSession(context, sessionId)) {
            return;
        }
        try {
            context.getPackageManager().getPackageInstaller().abandonSession(sessionId);
        } catch (RuntimeException ignoredFinishedSession) {
        }
        clearSession(context);
        setStatus(context, message);
    }

    static synchronized void setPendingUserConfirmation(
        Context context,
        int sessionId,
        Intent confirmation
    ) {
        if (isCurrentSession(context, sessionId)) {
            submittedSessionId = sessionId;
            pendingConfirmation = confirmation;
            confirmationLaunched = false;
        }
    }

    static synchronized boolean launchPendingUserConfirmation(Context context) {
        if (pendingConfirmation == null || confirmationLaunched) {
            return false;
        }
        try {
            context.startActivity(pendingConfirmation);
            confirmationLaunched = true;
            return true;
        } catch (RuntimeException error) {
            cancelSession(context, storedSessionId(context), "无法打开系统安装确认窗口, 请重试.");
            return false;
        }
    }
    // //// /保存安装状态并转交用户确认 ////

    // //// 创建或恢复游戏覆盖安装会话 [@x380kkm 2026-09-10] ////
    static synchronized void beginInstallIfNeeded(Context context) {
        if (isInstalled(context) || installScheduled || hasActiveSession(context)) {
            return;
        }
        installScheduled = true;
        setStatus(context, "正在准备游戏安装包...");
        Context applicationContext = context.getApplicationContext();
        INSTALL_EXECUTOR.execute(() -> createAndCommitSession(applicationContext));
    }

    static synchronized boolean isInstallInProgress(Context context) {
        return installScheduled || hasActiveSession(context);
    }

    private static void createAndCommitSession(Context context) {
        PackageInstaller.Session session = null;
        int sessionId = -1;
        try {
            PackageInstaller packageInstaller = context.getPackageManager().getPackageInstaller();
            PackageInstaller.SessionParams parameters =
                new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
            parameters.setAppPackageName(GAME_PACKAGE);
            long gameBytes = gameAssetLength(context);
            long versionCode = expectedGameVersionCode(context);
            parameters.setSize(gameBytes);
            if (Build.VERSION.SDK_INT >= 31) {
                parameters.setRequireUserAction(
                    PackageInstaller.SessionParams.USER_ACTION_REQUIRED
                );
            }
            sessionId = packageInstaller.createSession(parameters);
            boolean sessionRecorded = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putInt(SESSION_KEY, sessionId)
                .putLong(SESSION_VERSION_KEY, versionCode)
                .commit();
            if (!sessionRecorded) {
                throw new IOException("无法保存游戏安装进度, 请检查可用空间后重试.");
            }
            session = packageInstaller.openSession(sessionId);
            setStatus(context, "正在写入游戏安装包...");
            try (
                InputStream input = context.getAssets().open(GAME_ASSET);
                OutputStream output = session.openWrite("base.apk", 0, gameBytes)
            ) {
                copy(input, output);
                session.fsync(output);
            }
            setStatus(context, "请在系统窗口确认安装游戏...");
            synchronized (GameInstaller.class) {
                submittedSessionId = sessionId;
            }
            session.commit(createInstallIntentSender(context, sessionId));
        } catch (Exception error) {
            if (sessionId >= 0) {
                try {
                    context.getPackageManager().getPackageInstaller().abandonSession(sessionId);
                } catch (RuntimeException ignoredClosedSession) {
                }
            }
            clearSession(context);
            String message = error.getMessage();
            setStatus(
                context,
                message == null || message.trim().isEmpty()
                    ? "游戏安装准备失败, 请重试."
                    : "游戏安装准备失败: " + message
            );
        } finally {
            try {
                if (session != null) {
                    session.close();
                }
            } catch (RuntimeException error) {
                Log.w("StarpointPersonalService", "无法关闭游戏安装会话句柄.", error);
            } finally {
                synchronized (GameInstaller.class) {
                    installScheduled = false;
                }
            }
        }
    }

    private static IntentSender createInstallIntentSender(Context context, int sessionId) {
        Intent intent = new Intent(context, GameInstallReceiver.class);
        intent.setAction(INSTALL_RESULT_ACTION);
        intent.putExtra(PackageInstaller.EXTRA_SESSION_ID, sessionId);
        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= 31) {
            flags |= PendingIntent.FLAG_MUTABLE;
        }
        PendingIntent pendingIntent = PendingIntent.getBroadcast(
            context,
            sessionId,
            intent,
            flags
        );
        return pendingIntent.getIntentSender();
    }

    private static synchronized boolean hasActiveSession(Context context) {
        PackageInstaller packageInstaller = context.getPackageManager().getPackageInstaller();
        int sessionId = storedSessionId(context);
        if (sessionId < 0) {
            return false;
        }
        try {
            PackageInstaller.SessionInfo session = packageInstaller.getSessionInfo(sessionId);
            if (session == null) {
                clearSession(context);
                return false;
            }
            long version = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .getLong(SESSION_VERSION_KEY, -1L);
            if (version != expectedGameVersionCode(context)) {
                cancelSession(context, sessionId, "游戏安装包已更新, 请重新确认安装.");
                return false;
            }
            if (sessionId == submittedSessionId) {
                return true;
            }
            if (!session.isSealed()) {
                cancelSession(context, sessionId, "游戏安装准备被中断, 请重试.");
                return false;
            }
            submittedSessionId = sessionId;
            installScheduled = true;
            Context applicationContext = context.getApplicationContext();
            INSTALL_EXECUTOR.execute(() -> recommitSession(applicationContext, sessionId));
            return true;
        } catch (IOException | RuntimeException error) {
            cancelSession(context, sessionId, "无法恢复游戏安装会话, 请重试.");
        }
        return false;
    }

    private static void recommitSession(Context context, int sessionId) {
        try (
            PackageInstaller.Session session = context.getPackageManager()
                .getPackageInstaller().openSession(sessionId)
        ) {
            session.commit(createInstallIntentSender(context, sessionId));
        } catch (IOException | RuntimeException error) {
            cancelSession(context, sessionId, "无法恢复游戏安装确认, 请重试.");
        } finally {
            synchronized (GameInstaller.class) {
                installScheduled = false;
            }
        }
    }

    private static int storedSessionId(Context context) {
        return context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getInt(SESSION_KEY, -1);
    }

    private static void copy(InputStream input, OutputStream output) throws IOException {
        byte[] buffer = new byte[COPY_BUFFER_SIZE];
        int count;
        while ((count = input.read(buffer)) != -1) {
            output.write(buffer, 0, count);
        }
    }

    private static long gameAssetLength(Context context) throws IOException {
        try (AssetFileDescriptor descriptor = context.getAssets().openFd(GAME_ASSET)) {
            return descriptor.getLength();
        }
    }

    private static String sha256(byte[] bytes) {
        try {
            return hex(MessageDigest.getInstance("SHA-256").digest(bytes));
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("Android 缺少 SHA-256.", error);
        }
    }

    private static String hex(byte[] bytes) {
        StringBuilder result = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            result.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        }
        return result.toString();
    }
    // //// /创建或恢复游戏覆盖安装会话 ////

    private enum InstalledState {
        ABSENT,
        COMPATIBLE,
        UPDATE_REQUIRED,
        SIGNATURE_MISMATCH,
        UNVERIFIABLE
    }
}
