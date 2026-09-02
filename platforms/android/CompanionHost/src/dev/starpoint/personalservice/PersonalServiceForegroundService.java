// audience: external
// # android-personal-service-foreground-service
//
// 该服务维持进程内个人服务和常驻通知. START_STICKY 重建重新读取完整 CDN 标记并绑定固定端口.
// Activity 进入后台和任务移除时只提交状态, 服务继续运行.

package dev.starpoint.personalservice;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.os.IBinder;
import android.util.Log;

import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

public final class PersonalServiceForegroundService extends Service {
    private static final String LOG_TAG = "StarpointPersonalService";
    private static final String NOTIFICATION_CHANNEL = "starpoint-personal-service";
    private static final String ACTION_FLUSH =
        "dev.starpoint.personalservice.action.FLUSH";
    private static final int NOTIFICATION_ID = 17171;
    private static final long START_RETRY_SECONDS = 5;
    private static final long HEALTH_CHECK_SECONDS = 5;

    private static volatile String lastStartFailure;

    private final AtomicBoolean startScheduled = new AtomicBoolean();
    private ScheduledExecutorService serviceExecutor;
    private volatile boolean destroyed;

    // //// 由可见 Activity 启动持久前台服务 [@x380kkm 2026-08-31] ////
    static void ensureStarted(Context context) {
        Intent intent = new Intent(context, PersonalServiceForegroundService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent);
        } else {
            context.startService(intent);
        }
    }

    static void requestFlush(Context context) {
        Intent intent = new Intent(context, PersonalServiceForegroundService.class);
        intent.setAction(ACTION_FLUSH);
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent);
            } else {
                context.startService(intent);
            }
        } catch (IllegalStateException error) {
            Log.w(LOG_TAG, "Personal service flush request arrived outside the foreground window.", error);
        }
    }

    static String lastStartFailure() {
        return lastStartFailure;
    }
    // //// /由可见 Activity 启动持久前台服务 ////

    // //// 在前台服务生命周期内启动, 重试并提交个人服务 [@x380kkm 2026-09-01] ////
    @Override
    public void onCreate() {
        super.onCreate();
        serviceExecutor = Executors.newSingleThreadScheduledExecutor();
        createNotificationChannel();
        startForeground(NOTIFICATION_ID, buildNotification("正在启动个人服务"));
        scheduleStart(0);
        serviceExecutor.scheduleAtFixedRate(
            this::monitorPersonalService,
            HEALTH_CHECK_SECONDS,
            HEALTH_CHECK_SECONDS,
            TimeUnit.SECONDS
        );
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_FLUSH.equals(intent.getAction())) {
            scheduleStart(0);
            serviceExecutor.execute(this::flushState);
        } else {
            scheduleStart(0);
        }
        return START_STICKY;
    }

    @Override
    public void onTaskRemoved(Intent rootIntent) {
        serviceExecutor.execute(this::flushState);
        super.onTaskRemoved(rootIntent);
    }

    @Override
    public void onTrimMemory(int level) {
        if (level >= TRIM_MEMORY_UI_HIDDEN) {
            serviceExecutor.execute(this::flushState);
        }
        super.onTrimMemory(level);
    }

    @Override
    public void onDestroy() {
        destroyed = true;
        flushState();
        serviceExecutor.shutdownNow();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void scheduleStart(long delaySeconds) {
        if (destroyed || !startScheduled.compareAndSet(false, true)) {
            return;
        }
        serviceExecutor.schedule(this::startPersonalService, delaySeconds, TimeUnit.SECONDS);
    }

    private void startPersonalService() {
        startScheduled.set(false);
        if (destroyed) {
            return;
        }
        try {
            CompanionServiceHost.Endpoint endpoint =
                CompanionServiceHost.getOrCreate(this).start(this::updateNotification);
            if (destroyed) {
                return;
            }
            lastStartFailure = null;
            updateNotification("运行于 127.0.0.1:" + endpoint.port());
        } catch (RuntimeException | LinkageError error) {
            String message = error.getMessage();
            lastStartFailure = message == null ? "个人服务启动失败." : message;
            Log.e(LOG_TAG, "Personal service start failed.", error);
            if (destroyed) {
                return;
            }
            updateNotification(CdnAssetInstaller.status());
            scheduleStart(START_RETRY_SECONDS);
        }
    }

    // //// 定期探测并恢复失效的个人服务 [@x380kkm 2026-09-01] ////
    private void monitorPersonalService() {
        if (destroyed) {
            return;
        }
        try {
            CompanionServiceHost host = CompanionServiceHost.getOrCreate(this);
            if (!host.isHealthy()) {
                updateNotification("正在恢复个人服务");
                host.restart(this::updateNotification);
                updateNotification("运行于 127.0.0.1:17171");
            }
        } catch (RuntimeException | LinkageError error) {
            Log.w(LOG_TAG, "Personal service health check failed.", error);
            scheduleStart(0);
        }
    }
    // //// /定期探测并恢复失效的个人服务 ////

    private void flushState() {
        if (!CompanionServiceHost.getOrCreate(this).flush()) {
            Log.e(LOG_TAG, "Personal service checkpoint failed.");
        }
    }
    // //// /在前台服务生命周期内启动, 重试并提交个人服务 ////

    // //// 展示可返回管理页的持久通知 [@x380kkm 2026-08-31] ////
    private void createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return;
        }
        NotificationChannel channel = new NotificationChannel(
            NOTIFICATION_CHANNEL,
            "Starpoint 个人服务",
            NotificationManager.IMPORTANCE_LOW
        );
        channel.setDescription("保持本机游戏服务可用");
        channel.setShowBadge(false);
        getSystemService(NotificationManager.class).createNotificationChannel(channel);
    }

    private Notification buildNotification(String text) {
        Intent managementIntent = new Intent(this, ManagementActivity.class);
        managementIntent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP);
        int pendingIntentFlags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            pendingIntentFlags |= PendingIntent.FLAG_IMMUTABLE;
        }
        PendingIntent managementPendingIntent = PendingIntent.getActivity(
            this,
            0,
            managementIntent,
            pendingIntentFlags
        );
        Notification.Builder builder = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
            ? new Notification.Builder(this, NOTIFICATION_CHANNEL)
            : new Notification.Builder(this);
        return builder
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Starpoint 个人服务")
            .setContentText(text)
            .setContentIntent(managementPendingIntent)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOnlyAlertOnce(true)
            .setOngoing(true)
            .build();
    }

    private void updateNotification(String text) {
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.notify(NOTIFICATION_ID, buildNotification(text));
    }
    // //// /展示可返回管理页的持久通知 ////
}
