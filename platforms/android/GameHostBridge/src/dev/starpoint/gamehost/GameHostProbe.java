// audience: internal
// # game-host-probe
//
// 游戏进程在后台读取宿主 Binder 状态并确认回环 HTTP 可以响应.

package dev.starpoint.gamehost;

import android.os.IBinder;
import android.os.DeadObjectException;
import android.os.Parcel;
import android.os.RemoteException;

final class GameHostProbe {
    private GameHostProbe() {
    }

    // //// 读取受限 Binder 的服务状态 [@x380kkm 2026-09-10] ////
    static Result query(IBinder binder, int transaction) {
        Parcel request = Parcel.obtain();
        Parcel response = Parcel.obtain();
        try {
            request.writeInterfaceToken(GameHostProtocol.DESCRIPTOR);
            if (!binder.transact(transaction, request, response, 0)) {
                return failed("宿主版本没有提供游戏连接协议.");
            }
            response.readException();
            int status = response.readInt();
            String message = response.readString();
            if (status == GameHostProtocol.STATUS_READY
                && !GameHostHealth.isResponsive(GameHostProtocol.PERSONAL_SERVICE_PORT)) {
                return new Result(GameHostProtocol.STATUS_STARTING, "正在等待个人服务响应...");
            }
            return new Result(status, message == null ? "个人服务尚未就绪." : message);
        } catch (DeadObjectException error) {
            return new Result(GameHostProtocol.STATUS_STARTING, "个人服务进程正在恢复.");
        } catch (RemoteException | RuntimeException error) {
            String detail = error.getMessage();
            return new Result(
                GameHostProtocol.STATUS_STARTING,
                detail == null ? "个人服务进程正在恢复." : "个人服务进程正在恢复: " + detail
            );
        } finally {
            request.recycle();
            response.recycle();
        }
    }

    private static Result failed(String message) {
        return new Result(GameHostProtocol.STATUS_FAILED, message);
    }

    static final class Result {
        final int status;
        final String message;

        Result(int status, String message) {
            this.status = status;
            this.message = message;
        }
    }
    // //// /读取受限 Binder 的服务状态 ////
}
