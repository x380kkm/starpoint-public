// audience: external
// # game-connection-service
//
// Binder 调用者通过 UID 与内嵌游戏签名验证后请求服务启动或查询状态.
// onBind 仅交付 Binder, 前台服务管理实际运行生命周期.

package dev.starpoint.personalservice;

import android.app.Service;
import android.content.Intent;
import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;
import android.os.RemoteException;

import dev.starpoint.gamehost.GameHostProtocol;

public final class GameConnectionService extends Service {
    private final IBinder binder = new ConnectionBinder();

    // //// 提供经签名校验的服务状态接口 [@x380kkm 2026-09-10] ////
    @Override
    public IBinder onBind(Intent intent) {
        return binder;
    }

    private final class ConnectionBinder extends Binder {
        ConnectionBinder() {
            attachInterface(null, GameHostProtocol.DESCRIPTOR);
        }

        @Override
        protected boolean onTransact(int code, Parcel data, Parcel reply, int flags)
            throws RemoteException {
            if (code == INTERFACE_TRANSACTION) {
                if (reply != null) {
                    reply.writeString(GameHostProtocol.DESCRIPTOR);
                }
                return true;
            }
            if (code != GameHostProtocol.TRANSACTION_PREPARE
                && code != GameHostProtocol.TRANSACTION_STATUS) {
                return super.onTransact(code, data, reply, flags);
            }
            if (reply == null || (flags & IBinder.FLAG_ONEWAY) != 0) {
                return false;
            }
            data.enforceInterface(GameHostProtocol.DESCRIPTOR);
            if (!GameInstaller.isTrustedGameCaller(GameConnectionService.this, Binder.getCallingUid())) {
                writeStatus(reply, GameHostProtocol.STATUS_DENIED, "游戏调用者未通过签名验证.");
                return true;
            }
            long identity = Binder.clearCallingIdentity();
            try {
                CompanionServiceHost host =
                    CompanionServiceHost.getOrCreate(GameConnectionService.this);
                if (code == GameHostProtocol.TRANSACTION_PREPARE || !host.isRunning()) {
                    PersonalServiceForegroundService.ensureStarted(GameConnectionService.this);
                }
                if (host.isRunning()) {
                    writeStatus(reply, GameHostProtocol.STATUS_READY, "个人服务已就绪.");
                } else {
                    String failure = PersonalServiceForegroundService.lastStartFailure();
                    boolean needsResourceDocument = CdnAssetInstaller.requiresResourceDocument();
                    writeStatus(
                        reply,
                        failure == null || !needsResourceDocument
                            ? GameHostProtocol.STATUS_STARTING
                            : GameHostProtocol.STATUS_FAILED,
                        failure == null ? CdnAssetInstaller.status() : failure
                    );
                }
            } catch (RuntimeException | LinkageError error) {
                String detail = error.getMessage();
                writeStatus(reply, GameHostProtocol.STATUS_STARTING,
                    detail == null ? "个人服务启动失败." : detail);
            } finally {
                Binder.restoreCallingIdentity(identity);
            }
            return true;
        }

        private void writeStatus(Parcel reply, int status, String message) {
            reply.writeNoException();
            reply.writeInt(status);
            reply.writeString(message);
        }
    }
    // //// /提供经签名校验的服务状态接口 ////
}
