// audience: external
// # game-launch-activity
//
// 游戏入口先等待个人服务, 恢复时通过结束当前 Activity 返回已有游戏实例.
// 独立启动只进入固定 AppEntry, 外部 Intent 的组件和 extras 保持在当前页面.

package dev.starpoint.gamehost;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.os.Bundle;
import android.provider.Settings;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

public final class GameLaunchActivity extends Activity {
    private TextView statusView;
    private Button retryButton;
    private boolean visible;
    private boolean enteredGame;

    // //// 显示可重试的服务等待入口 [@x380kkm 2026-09-10] ////
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        GameHostConnection.install(getApplication());
        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setGravity(Gravity.CENTER);
        int padding = (int)(24 * getResources().getDisplayMetrics().density);
        content.setPadding(padding, padding, padding, padding);
        statusView = new TextView(this);
        statusView.setTextSize(17);
        statusView.setGravity(Gravity.CENTER);
        content.addView(statusView, new LinearLayout.LayoutParams(-1, -2));
        retryButton = new Button(this);
        retryButton.setText("重试连接");
        retryButton.setOnClickListener(view -> connect());
        content.addView(retryButton, new LinearLayout.LayoutParams(-1, -2));
        Button hostButton = new Button(this);
        hostButton.setText("打开 Starpoint");
        hostButton.setOnClickListener(view -> openHost());
        content.addView(hostButton, new LinearLayout.LayoutParams(-1, -2));
        setContentView(content);
    }

    @Override
    protected void onResume() {
        super.onResume();
        visible = true;
        if (!enteredGame) {
            connect();
        }
    }

    @Override
    protected void onPause() {
        visible = false;
        GameHostConnection.pauseGateway(this);
        super.onPause();
    }

    @Override
    public void onBackPressed() {
        moveTaskToBack(true);
    }

    private void connect() {
        statusView.setText("正在连接 Starpoint 个人服务...");
        retryButton.setEnabled(false);
        GameHostConnection.awaitReady(this, (ready, message, complete) -> {
            if (!visible || isFinishing() || isDestroyed() || enteredGame) {
                return;
            }
            statusView.setText(message);
            retryButton.setEnabled(complete);
            if (ready) {
                enterGame();
            }
        });
    }
    // //// /显示可重试的服务等待入口 ////

    // //// 返回已有游戏或进入固定游戏组件 [@x380kkm 2026-09-10] ////
    private void enterGame() {
        enteredGame = true;
        if (GameHostConnection.finishGateway(this)) {
            finish();
            return;
        }
        Intent intent = new Intent(Intent.ACTION_MAIN).setComponent(new ComponentName(
            getPackageName(), GameHostProtocol.GAME_ACTIVITY));
        intent.addFlags(Intent.FLAG_ACTIVITY_REORDER_TO_FRONT | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        try {
            startActivity(intent);
            finish();
        } catch (RuntimeException error) {
            enteredGame = false;
            statusView.setText("无法打开游戏入口: " + error.getMessage());
            retryButton.setEnabled(true);
        }
    }

    private void openHost() {
        Intent intent = new Intent().setComponent(new ComponentName(
            GameHostProtocol.HOST_PACKAGE,
            GameHostProtocol.HOST_PACKAGE + ".ManagementActivity"));
        try {
            startActivity(intent);
        } catch (RuntimeException missingHost) {
            statusView.setText("请安装完整的 Starpoint 分发包, 然后返回此页面重试. 游戏存档保持原状.");
            try {
                startActivity(new Intent(Settings.ACTION_APPLICATION_SETTINGS));
            } catch (RuntimeException unavailableSettings) {
                retryButton.setEnabled(true);
            }
        }
    }
    // //// /返回已有游戏或进入固定游戏组件 ////
}
