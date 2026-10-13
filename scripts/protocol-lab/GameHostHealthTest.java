// audience: internal
// # game-host-health-test
//
// 此测试通过真实回环连接验证宿主不可用, 响应阻塞和恢复后的就绪判断.

package dev.starpoint.gamehost;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

public final class GameHostHealthTest {
    // //// 验证服务状态只由当前 HTTP 响应决定 [@x380kkm 2026-10-02] ////
    public static void main(String[] arguments) throws Exception {
        int port;
        try (ServerSocket listener = listen(0)) {
            port = listener.getLocalPort();
            assertResponse(listener, 503, false);
            assertResponse(listener, 200, true);
            assertBlockedResponse(listener);
            assertResponse(listener, 200, true);
        }
        require(!GameHostHealth.isResponsive(port), "closed service remains unavailable");
        try (ServerSocket listener = listen(port)) {
            assertResponse(listener, 200, true);
        }
        System.out.println("宿主探测验证完成: 服务错误, 阻塞, 断开和重启恢复.");
    }

    private static ServerSocket listen(int port) throws IOException {
        ServerSocket listener = new ServerSocket();
        listener.setReuseAddress(true);
        listener.bind(new java.net.InetSocketAddress(InetAddress.getByName("127.0.0.1"), port));
        listener.setSoTimeout(5_000);
        return listener;
    }

    private static void assertResponse(ServerSocket listener, int status, boolean expected) throws Exception {
        AtomicReference<Throwable> failure = new AtomicReference<>();
        Thread server = new Thread(() -> {
            try (Socket socket = listener.accept()) {
                readRequest(socket);
                String response = "HTTP/1.1 " + status + " Result\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                socket.getOutputStream().write(response.getBytes(StandardCharsets.US_ASCII));
            } catch (Throwable error) {
                failure.set(error);
            }
        });
        server.start();
        boolean actual = GameHostHealth.isResponsive(listener.getLocalPort());
        server.join(6_000);
        require(!server.isAlive() && failure.get() == null, "response server finishes: " + failure.get());
        require(actual == expected, "HTTP status " + status + " readiness matches");
    }

    private static void assertBlockedResponse(ServerSocket listener) throws Exception {
        CountDownLatch release = new CountDownLatch(1);
        AtomicReference<Throwable> failure = new AtomicReference<>();
        Thread server = new Thread(() -> {
            try (Socket socket = listener.accept()) {
                readRequest(socket);
                release.await(5, TimeUnit.SECONDS);
            } catch (Throwable error) {
                failure.set(error);
            }
        });
        server.start();
        try {
            require(!GameHostHealth.isResponsive(listener.getLocalPort()),
                "an open socket without an HTTP response is not ready");
        } finally {
            release.countDown();
            server.join(6_000);
        }
        require(!server.isAlive() && failure.get() == null, "blocked server finishes: " + failure.get());
    }

    private static void readRequest(Socket socket) throws IOException {
        socket.setSoTimeout(5_000);
        BufferedReader input = new BufferedReader(new InputStreamReader(
            socket.getInputStream(), StandardCharsets.US_ASCII));
        String line;
        while ((line = input.readLine()) != null && !line.isEmpty()) {
        }
    }

    private static void require(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError(message);
        }
    }
    // //// /验证服务状态只由当前 HTTP 响应决定 ////
}
