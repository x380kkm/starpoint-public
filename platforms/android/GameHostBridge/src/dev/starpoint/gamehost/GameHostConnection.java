// audience: external
// # game-host-connection
//
// install 由游戏 Application.onCreate 调用, 仅登记 Activity 生命周期监听器.
// 主线程拥有连接状态, Binder 请求在单独线程执行, 跨包绑定随游戏进程保留.

package dev.starpoint.gamehost;

import android.app.Activity;
import android.app.Application;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.RemoteException;
import android.os.SystemClock;

import java.lang.ref.WeakReference;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class GameHostConnection implements Application.ActivityLifecycleCallbacks {
    private static final long WAIT_MILLISECONDS = 120_000;
    private static final long POLL_MILLISECONDS = 500;
    private static GameHostConnection instance;

    private final Application application;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final ExecutorService requests = Executors.newSingleThreadExecutor();
    private WeakReference<Activity> gameActivity = new WeakReference<>(null);
    private WeakReference<Activity> resumedActivity = new WeakReference<>(null);
    private WeakReference<GameLaunchActivity> gateway = new WeakReference<>(null);
    private IBinder binder;
    private IBinder.DeathRecipient deathRecipient;
    private boolean bound;
    private boolean ready;
    private boolean gatewayOpening;
    private boolean prepareRequested;
    private boolean requestInFlight;
    private int generation;
    private long deadline;
    private ReadyListener listener;
    private final Runnable poll = this::query;
    private final Runnable healthPoll = this::verifyResumedGame;
    private final Runnable timeout = () -> fail("等待个人服务超时. 请查看 Starpoint 中的启动状态后重试.");

    private final ServiceConnection serviceConnection = new ServiceConnection() {
        @Override
        public void onServiceConnected(ComponentName name, IBinder service) {
            clearBinder();
            binder = service;
            ready = false;
            prepareRequested = false;
            deathRecipient = () -> main.post(() -> {
                if (binder == service) {
                    disconnected();
                }
            });
            try {
                service.linkToDeath(deathRecipient, 0);
            } catch (RemoteException error) {
                disconnected();
                return;
            }
            query();
            Activity current = resumedActivity.get();
            if (listener == null && current != null) {
                showGateway(current);
            }
        }

        @Override
        public void onServiceDisconnected(ComponentName name) {
            disconnected();
        }

        @Override
        public void onBindingDied(ComponentName name) {
            releaseBinding();
            disconnected();
            ensureBound();
        }

        @Override
        public void onNullBinding(ComponentName name) {
            releaseBinding();
            fail("当前 Starpoint 包没有提供游戏连接接口, 请覆盖安装配套版本.");
        }
    };

    private GameHostConnection(Application application) {
        this.application = application;
    }

    // //// 登记游戏生命周期与等待界面 [@x380kkm 2026-09-10] ////
    public static synchronized void install(Application application) {
        if (instance == null) {
            instance = new GameHostConnection(application);
            application.registerActivityLifecycleCallbacks(instance);
        }
    }

    static void awaitReady(GameLaunchActivity activity, ReadyListener listener) {
        install(activity.getApplication());
        GameHostConnection connection = instance;
        connection.cancelWait();
        connection.gateway = new WeakReference<>(activity);
        connection.gatewayOpening = true;
        connection.listener = listener;
        connection.deadline = SystemClock.elapsedRealtime() + WAIT_MILLISECONDS;
        connection.prepareRequested = false;
        connection.ready = false;
        connection.main.postDelayed(connection.timeout, WAIT_MILLISECONDS);
        if (connection.ensureBound()) {
            connection.query();
        }
    }

    static void pauseGateway(GameLaunchActivity activity) {
        if (instance != null && instance.gateway.get() == activity) {
            instance.cancelWait();
        }
    }

    static boolean finishGateway(GameLaunchActivity activity) {
        GameHostConnection connection = instance;
        if (connection == null) {
            return false;
        }
        connection.cancelWait();
        connection.gatewayOpening = false;
        connection.gateway.clear();
        Activity previous = connection.gameActivity.get();
        return previous != null && !previous.isFinishing() && !previous.isDestroyed()
            && previous.getTaskId() == activity.getTaskId();
    }
    // //// /登记游戏生命周期与等待界面 ////

    // //// 保持宿主绑定并在断开时保护前台游戏 [@x380kkm 2026-09-10] ////
    private boolean ensureBound() {
        if (bound) {
            return true;
        }
        Intent intent = new Intent().setComponent(new ComponentName(
            GameHostProtocol.HOST_PACKAGE, GameHostProtocol.HOST_SERVICE));
        try {
            bound = application.bindService(intent, serviceConnection,
                Context.BIND_AUTO_CREATE | Context.BIND_IMPORTANT | Context.BIND_ABOVE_CLIENT);
        } catch (RuntimeException error) {
            fail("无法连接 Starpoint 个人服务: " + error.getMessage());
            return false;
        }
        if (!bound) {
            fail("请先安装并打开配套的 Starpoint 分发包.");
        }
        return bound;
    }

    private void disconnected() {
        clearBinder();
        ready = false;
        prepareRequested = false;
        Activity current = resumedActivity.get();
        if (current != null) {
            showGateway(current);
        }
        if (listener != null) {
            listener.onState(false, "个人服务连接中断, 正在等待恢复.", false);
            scheduleQuery();
        }
    }

    private void clearBinder() {
        generation++;
        main.removeCallbacks(healthPoll);
        if (binder != null && deathRecipient != null) {
            try {
                binder.unlinkToDeath(deathRecipient, 0);
            } catch (java.util.NoSuchElementException ignored) {
            }
        }
        binder = null;
        deathRecipient = null;
    }

    private void releaseBinding() {
        clearBinder();
        if (bound) {
            bound = false;
            try {
                application.unbindService(serviceConnection);
            } catch (IllegalArgumentException ignored) {
            }
        }
    }
    // //// /保持宿主绑定并在断开时保护前台游戏 ////

    // //// 在后台查询服务并有界等待就绪 [@x380kkm 2026-09-10] ////
    private void query() {
        main.removeCallbacks(poll);
        if (listener == null || requestInFlight) {
            return;
        }
        if (SystemClock.elapsedRealtime() >= deadline) {
            timeout.run();
            return;
        }
        if (binder == null) {
            scheduleQuery();
            return;
        }
        requestInFlight = true;
        int requestGeneration = generation;
        IBinder target = binder;
        int code = prepareRequested ? GameHostProtocol.TRANSACTION_STATUS : GameHostProtocol.TRANSACTION_PREPARE;
        prepareRequested = true;
        requests.execute(() -> {
            GameHostProbe.Result result = GameHostProbe.query(target, code);
            main.post(() -> {
                requestInFlight = false;
                if (requestGeneration != generation || target != binder) {
                    query();
                    return;
                }
                applyResult(result);
            });
        });
    }

    private void applyResult(GameHostProbe.Result result) {
        if (listener == null) {
            return;
        }
        if (result.status == GameHostProtocol.STATUS_READY) {
            ready = true;
            main.removeCallbacks(timeout);
            listener.onState(true, result.message, true);
        } else if (result.status == GameHostProtocol.STATUS_STARTING) {
            listener.onState(false, result.message, false);
            scheduleQuery();
        } else {
            fail(result.message);
        }
    }

    private void scheduleQuery() {
        main.removeCallbacks(poll);
        if (listener != null) {
            main.postDelayed(poll, POLL_MILLISECONDS);
        }
    }

    private void fail(String message) {
        ReadyListener current = listener;
        ready = false;
        cancelWait();
        if (current != null) {
            current.onState(false, message, true);
        }
    }

    private void cancelWait() {
        generation++;
        listener = null;
        main.removeCallbacks(poll);
        main.removeCallbacks(healthPoll);
        main.removeCallbacks(timeout);
    }
    // //// /在后台查询服务并有界等待就绪 ////

    // //// 在游戏前台期间检查宿主并等待连接恢复 [@x380kkm 2026-10-02] ////
    private void verifyResumedGame() {
        main.removeCallbacks(healthPoll);
        Activity activity = resumedActivity.get();
        if (activity == null || listener != null || gatewayOpening || binder == null) {
            return;
        }
        if (requestInFlight) {
            scheduleHealthQuery();
            return;
        }
        requestInFlight = true;
        IBinder target = binder;
        int requestGeneration = generation;
        requests.execute(() -> {
            GameHostProbe.Result result = GameHostProbe.query(target, GameHostProtocol.TRANSACTION_STATUS);
            main.post(() -> {
                requestInFlight = false;
                if (listener != null) {
                    query();
                    return;
                }
                if (requestGeneration != generation || target != binder
                    || activity != resumedActivity.get()) {
                    verifyResumedGame();
                    return;
                }
                ready = result.status == GameHostProtocol.STATUS_READY;
                if (!ready) {
                    showGateway(activity);
                } else {
                    scheduleHealthQuery();
                }
            });
        });
    }

    private void scheduleHealthQuery() {
        main.removeCallbacks(healthPoll);
        if (resumedActivity.get() != null && listener == null && !gatewayOpening) {
            main.postDelayed(healthPoll, POLL_MILLISECONDS);
        }
    }
    // //// /在游戏前台期间检查宿主并等待连接恢复 ////

    // //// 用等待界面暂停宿主不可用时的游戏 [@x380kkm 2026-09-10] ////
    private void showGateway(Activity activity) {
        if (gatewayOpening || activity.isFinishing() || activity.isDestroyed()) {
            return;
        }
        gatewayOpening = true;
        try {
            activity.startActivity(new Intent(activity, GameLaunchActivity.class)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP));
        } catch (RuntimeException error) {
            gatewayOpening = false;
        }
    }

    @Override
    public void onActivityResumed(Activity activity) {
        if (activity instanceof GameLaunchActivity) {
            return;
        }
        gameActivity = new WeakReference<>(activity);
        resumedActivity = new WeakReference<>(activity);
        if (!ready || binder == null || !binder.isBinderAlive()) {
            showGateway(activity);
        } else {
            verifyResumedGame();
        }
    }

    @Override
    public void onActivityCreated(Activity activity, Bundle state) {
        if (!(activity instanceof GameLaunchActivity)) {
            gameActivity = new WeakReference<>(activity);
        }
    }

    @Override
    public void onActivityDestroyed(Activity activity) {
        if (gameActivity.get() == activity) {
            gameActivity.clear();
        }
        if (resumedActivity.get() == activity) {
            resumedActivity.clear();
            main.removeCallbacks(healthPoll);
        }
        if (gateway.get() == activity) {
            gatewayOpening = false;
            gateway.clear();
            cancelWait();
        }
    }

    @Override public void onActivityStarted(Activity activity) { }
    @Override public void onActivityPaused(Activity activity) {
        if (resumedActivity.get() == activity) {
            resumedActivity.clear();
            main.removeCallbacks(healthPoll);
        }
    }
    @Override public void onActivityStopped(Activity activity) { }
    @Override public void onActivitySaveInstanceState(Activity activity, Bundle state) { }

    interface ReadyListener {
        void onState(boolean ready, String message, boolean complete);
    }
    // //// /用等待界面暂停宿主不可用时的游戏 ////
}
