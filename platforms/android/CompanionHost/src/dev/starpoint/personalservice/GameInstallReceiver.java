// audience: internal
// # android-game-install-receiver
//
// 该接收器接收 PackageInstaller 的状态并把系统确认窗口交给用户.

package dev.starpoint.personalservice;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.os.Build;

public final class GameInstallReceiver extends BroadcastReceiver {
    // //// 转交系统安装确认并记录安装结果 [@x380kkm 2026-08-31] ////
    @Override
    public void onReceive(Context context, Intent intent) {
        if (!GameInstaller.INSTALL_RESULT_ACTION.equals(intent.getAction())) {
            return;
        }
        int status = intent.getIntExtra(
            PackageInstaller.EXTRA_STATUS,
            PackageInstaller.STATUS_FAILURE
        );
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            showUserConfirmation(context, intent);
            return;
        }
        GameInstaller.clearSession(context);
        if (status == PackageInstaller.STATUS_SUCCESS) {
            GameInstaller.setStatus(context, "游戏安装完成, 正在启动个人服务...");
            openManagementActivity(context, true, null);
            return;
        }
        String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
        if (message == null || message.trim().isEmpty()) {
            message = "系统拒绝了游戏安装, 请重试.";
        }
        GameInstaller.setStatus(context, "游戏安装失败: " + message);
        openManagementActivity(context, false, message);
    }

    private static void showUserConfirmation(Context context, Intent resultIntent) {
        GameInstaller.setStatus(context, "请在系统窗口确认安装游戏...");
        Intent confirmation = getConfirmationIntent(resultIntent);
        if (confirmation == null) {
            GameInstaller.setStatus(context, "系统没有提供游戏安装确认窗口, 请重试.");
            return;
        }
        confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            context.startActivity(confirmation);
        } catch (RuntimeException error) {
            GameInstaller.setStatus(context, "无法打开系统安装确认窗口, 请重试.");
        }
    }

    private static Intent getConfirmationIntent(Intent resultIntent) {
        if (Build.VERSION.SDK_INT >= 33) {
            return resultIntent.getParcelableExtra(
                Intent.EXTRA_INTENT,
                Intent.class
            );
        }
        return (Intent) resultIntent.getParcelableExtra(Intent.EXTRA_INTENT);
    }

    private static void openManagementActivity(
        Context context,
        boolean succeeded,
        String failure
    ) {
        Intent activity = new Intent(context, ManagementActivity.class);
        activity.addFlags(
            Intent.FLAG_ACTIVITY_NEW_TASK
                | Intent.FLAG_ACTIVITY_CLEAR_TOP
                | Intent.FLAG_ACTIVITY_SINGLE_TOP
        );
        activity.putExtra(GameInstaller.EXTRA_INSTALL_SUCCEEDED, succeeded);
        if (failure != null) {
            activity.putExtra(GameInstaller.EXTRA_INSTALL_FAILURE, failure);
        }
        try {
            context.startActivity(activity);
        } catch (RuntimeException error) {
            GameInstaller.setStatus(
                context,
                succeeded
                    ? "游戏安装完成, 请打开 Starpoint 继续启动游戏."
                    : "游戏安装失败, 请打开 Starpoint 重试."
            );
        }
    }
    // //// /转交系统安装确认并记录安装结果 ////
}
