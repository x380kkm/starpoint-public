// audience: internal
// # game-host-health
//
// 本地 HTTP 就绪探测使用独立连接和有界等待, 响应仅表示服务可以处理请求.

package dev.starpoint.gamehost;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;

final class GameHostHealth {
    private static final int RESPONSE_TIMEOUT_MILLISECONDS = 2_000;

    private GameHostHealth() {
    }

    // //// 通过回环 HTTP 响应确认服务就绪 [@x380kkm 2026-10-02] ////
    static boolean isResponsive(int port) {
        try (Socket connection = new Socket()) {
            connection.connect(new InetSocketAddress("127.0.0.1", port), RESPONSE_TIMEOUT_MILLISECONDS);
            connection.setSoTimeout(RESPONSE_TIMEOUT_MILLISECONDS);
            connection.getOutputStream().write(("GET /health HTTP/1.1\r\n"
                + "Host: 127.0.0.1\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
            BufferedReader input = new BufferedReader(new InputStreamReader(
                connection.getInputStream(), StandardCharsets.US_ASCII));
            String status = input.readLine();
            char[] response = new char[256];
            while (input.read(response) != -1) {
            }
            return status != null && status.startsWith("HTTP/1.1 200 ");
        } catch (IOException failure) {
            return false;
        }
    }
    // //// /通过回环 HTTP 响应确认服务就绪 ////
}
