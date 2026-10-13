// audience: external
// # game-host-protocol
//
// 游戏与宿主共同编译此协议, 内容仅包含服务启动请求和可用状态.

package dev.starpoint.gamehost;

public final class GameHostProtocol {
    // //// 定义受限宿主连接协议 [@x380kkm 2026-09-10] ////
    public static final String HOST_PACKAGE = "dev.starpoint.personalservice";
    public static final String HOST_SERVICE =
        HOST_PACKAGE + ".GameConnectionService";
    public static final String GAME_PACKAGE = "com.leiting.wf";
    public static final String DESCRIPTOR = HOST_PACKAGE + ".game";
    public static final String GAME_ACTIVITY = "air.com.leiting.wf.AppEntry";
    public static final int PERSONAL_SERVICE_PORT = 17171;
    public static final int TRANSACTION_PREPARE = 1;
    public static final int TRANSACTION_STATUS = 2;
    public static final int STATUS_READY = 1;
    public static final int STATUS_STARTING = 0;
    public static final int STATUS_DENIED = -1;
    public static final int STATUS_FAILED = -2;

    private GameHostProtocol() {
    }
    // //// /定义受限宿主连接协议 ////
}
