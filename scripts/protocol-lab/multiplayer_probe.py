# audience: external
# # multiplayer-probe
# 此程序同时监听 TCP 和 UDP, 并把每个客户端数据块原样保存为独立二进制文件和 JSONL 事件.
# 此程序另外解析 TCP 中以 NUL 结尾的 JSON 帧和握手字段, 但不向客户端发送响应.

from __future__ import annotations

import argparse
import asyncio
import hashlib
import itertools
import json
import signal
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


MAX_JSON_FRAME_BYTES = 4 * 1024 * 1024


# //// 表示探针启动所需的不可变配置 [@x380kkm 2026-07-20] ////
@dataclass(frozen=True)
class ProbeConfig:
    listen_host: str
    port: int
    output_directory: Path


# //// /表示探针启动所需的不可变配置 ////


# //// 表示从 TCP 字节流中解析出的一个完整 JSON 帧 [@x380kkm 2026-07-20] ////
@dataclass(frozen=True)
class DecodedJsonFrame:
    payload: bytes
    value: Any | None
    error: str | None


# //// /表示从 TCP 字节流中解析出的一个完整 JSON 帧 ////


# //// 从任意 TCP 数据块中恢复 NUL 分隔的 JSON 帧 [@x380kkm 2026-07-20] ////
class NulJsonFrameDecoder:
    def __init__(self, max_frame_bytes: int = MAX_JSON_FRAME_BYTES) -> None:
        if max_frame_bytes <= 0:
            raise ValueError("JSON 帧上限必须大于 0.")
        self._max_frame_bytes = max_frame_bytes
        self._buffer = bytearray()

    def feed(self, payload: bytes) -> list[DecodedJsonFrame]:
        self._buffer.extend(payload)
        frames: list[DecodedJsonFrame] = []
        while True:
            boundary = self._buffer.find(0)
            if boundary < 0:
                if len(self._buffer) > self._max_frame_bytes:
                    raise ValueError(f"JSON 帧超过 {self._max_frame_bytes} 字节上限.")
                return frames
            if boundary > self._max_frame_bytes:
                raise ValueError(f"JSON 帧超过 {self._max_frame_bytes} 字节上限.")

            frame_payload = bytes(self._buffer[:boundary])
            del self._buffer[: boundary + 1]
            frames.append(_decode_json_frame(frame_payload))

    def remainder(self) -> bytes:
        return bytes(self._buffer)


# //// /从任意 TCP 数据块中恢复 NUL 分隔的 JSON 帧 ////


# //// 解析一个完整帧并保留可诊断的解码错误 [@x380kkm 2026-07-20] ////
def _decode_json_frame(payload: bytes) -> DecodedJsonFrame:
    try:
        value = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        return DecodedJsonFrame(payload=payload, value=None, error=str(error))
    return DecodedJsonFrame(payload=payload, value=value, error=None)


# //// /解析一个完整帧并保留可诊断的解码错误 ////


_HANDSHAKE_FIELDS_BY_SOCKLET = {
    "cooperation_room": {
        "reconnected",
        "socklet",
        "viewerId",
        "roomNumber",
        "questCategory",
        "questId",
    },
    "cooperation_battle": {
        "reconnected",
        "socklet",
        "connectionId",
        "roomNumber",
    },
}


# //// 检查客户端握手所属通道和必需字段 [@x380kkm 2026-07-20] ////
def assess_handshake(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict) or "socklet" not in value:
        return None

    socklet = value["socklet"]
    required_fields = _HANDSHAKE_FIELDS_BY_SOCKLET.get(socklet)
    if required_fields is None:
        return {
            "socklet": socklet,
            "recognized": False,
            "valid": False,
            "missing_fields": [],
        }

    missing_fields = sorted(required_fields.difference(value))
    return {
        "socklet": socklet,
        "recognized": True,
        "valid": not missing_fields,
        "missing_fields": missing_fields,
    }


# //// /检查客户端握手所属通道和必需字段 ////


# //// 持久保存协议事件和原始数据块 [@x380kkm 2026-07-20] ////
class CaptureStore:
    def __init__(self, output_directory: Path) -> None:
        self.output_directory = output_directory
        self.payload_directory = output_directory / "payloads"
        self.event_path = output_directory / "events.jsonl"
        self._sequence = itertools.count(1)
        self._lock = asyncio.Lock()

    def prepare(self) -> None:
        self.payload_directory.mkdir(parents=True, exist_ok=True)

    async def record_event(self, event: str, **fields: Any) -> None:
        async with self._lock:
            self._append_event({"event": event, **fields})

    async def record_payload(
        self,
        transport: str,
        peer: tuple[str, int] | Any,
        payload: bytes,
    ) -> None:
        async with self._lock:
            sequence = next(self._sequence)
            file_name = f"{sequence:08d}-{transport}.bin"
            payload_path = self.payload_directory / file_name
            payload_path.write_bytes(payload)
            self._append_event(
                {
                    "event": "payload",
                    "transport": transport,
                    "peer": _render_peer(peer),
                    "bytes": len(payload),
                    "sha256": hashlib.sha256(payload).hexdigest(),
                    "prefix_hex": payload[:64].hex(),
                    "payload_path": str(payload_path.resolve()),
                }
            )

    async def record_json_frame(
        self,
        peer: tuple[str, int] | Any,
        frame: DecodedJsonFrame,
    ) -> None:
        event: dict[str, Any] = {
            "event": "tcp_json_frame",
            "peer": _render_peer(peer),
            "bytes": len(frame.payload),
            "sha256": hashlib.sha256(frame.payload).hexdigest(),
        }
        if frame.error is None:
            event["message"] = frame.value
            handshake = assess_handshake(frame.value)
            if handshake is not None:
                event["handshake"] = handshake
        else:
            event["decode_error"] = frame.error

        async with self._lock:
            self._append_event(event)

    def _append_event(self, event: dict[str, Any]) -> None:
        complete_event = {
            "timestamp_utc": datetime.now(UTC).isoformat(),
            **event,
        }
        with self.event_path.open("a", encoding="utf-8", newline="\n") as output:
            output.write(json.dumps(complete_event, ensure_ascii=False) + "\n")


# //// /持久保存协议事件和原始数据块 ////


# //// 表示正在监听的 TCP 和 UDP 资源 [@x380kkm 2026-07-20] ////
@dataclass
class ProbeRuntime:
    config: ProbeConfig
    port: int
    store: CaptureStore
    tcp_server: asyncio.Server
    udp_transport: asyncio.DatagramTransport
    pending_tasks: set[asyncio.Task[None]]
    tcp_tasks: set[asyncio.Task[None]]

    async def close(self) -> None:
        self.tcp_server.close()
        await self.tcp_server.wait_closed()
        self.udp_transport.close()
        for task in self.tcp_tasks:
            if not task.done():
                task.cancel()
        if self.tcp_tasks:
            await asyncio.gather(*self.tcp_tasks, return_exceptions=True)
        if self.pending_tasks:
            await asyncio.gather(*self.pending_tasks, return_exceptions=True)
        await self.store.record_event("stopped")


# //// /表示正在监听的 TCP 和 UDP 资源 ////


# //// 记录 UDP 数据报并等待异步写入完成 [@x380kkm 2026-07-20] ////
class DatagramCaptureProtocol(asyncio.DatagramProtocol):
    def __init__(self, store: CaptureStore, pending_tasks: set[asyncio.Task[None]]) -> None:
        self.store = store
        self.pending_tasks = pending_tasks

    def datagram_received(self, data: bytes, address: tuple[str, int]) -> None:
        task = asyncio.create_task(self.store.record_payload("udp", address, data))
        self.pending_tasks.add(task)
        task.add_done_callback(self.pending_tasks.discard)

    def error_received(self, error: Exception) -> None:
        task = asyncio.create_task(self.store.record_event("udp_error", error=str(error)))
        self.pending_tasks.add(task)
        task.add_done_callback(self.pending_tasks.discard)


# //// /记录 UDP 数据报并等待异步写入完成 ////


# //// 把 socket peer 转换为稳定的 JSON 数据 [@x380kkm 2026-07-20] ////
def _render_peer(peer: tuple[str, int] | Any) -> dict[str, Any]:
    if isinstance(peer, tuple) and len(peer) >= 2:
        return {"host": str(peer[0]), "port": int(peer[1])}
    return {"value": str(peer)}


# //// /把 socket peer 转换为稳定的 JSON 数据 ////


# //// 读取一个 TCP 连接发送的全部客户端数据 [@x380kkm 2026-07-20] ////
async def _capture_tcp_connection(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    store: CaptureStore,
) -> None:
    peer = writer.get_extra_info("peername")
    decoder = NulJsonFrameDecoder()
    await store.record_event("tcp_connected", peer=_render_peer(peer))
    try:
        while payload := await reader.read(65536):
            await store.record_payload("tcp", peer, payload)
            for frame in decoder.feed(payload):
                await store.record_json_frame(peer, frame)
    except Exception as error:
        await store.record_event("tcp_error", peer=_render_peer(peer), error=str(error))
    finally:
        remainder = decoder.remainder()
        if remainder:
            await store.record_event(
                "tcp_incomplete_frame",
                peer=_render_peer(peer),
                bytes=len(remainder),
                sha256=hashlib.sha256(remainder).hexdigest(),
                prefix_hex=remainder[:64].hex(),
            )
        writer.close()
        await writer.wait_closed()
        await store.record_event("tcp_disconnected", peer=_render_peer(peer))


# //// /读取一个 TCP 连接发送的全部客户端数据 ////


# //// 启动一个由运行时负责回收的 TCP 捕获任务 [@x380kkm 2026-07-20] ////
def _start_tcp_capture_task(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    store: CaptureStore,
    tcp_tasks: set[asyncio.Task[None]],
) -> None:
    task = asyncio.create_task(_capture_tcp_connection(reader, writer, store))
    tcp_tasks.add(task)
    task.add_done_callback(tcp_tasks.discard)


# //// /启动一个由运行时负责回收的 TCP 捕获任务 ////


# //// 启动共享端口的 TCP 和 UDP 探针 [@x380kkm 2026-07-20] ////
async def start_probe(config: ProbeConfig) -> ProbeRuntime:
    store = CaptureStore(config.output_directory)
    store.prepare()
    pending_tasks: set[asyncio.Task[None]] = set()
    tcp_tasks: set[asyncio.Task[None]] = set()
    tcp_server = await asyncio.start_server(
        lambda reader, writer: _start_tcp_capture_task(
            reader,
            writer,
            store,
            tcp_tasks,
        ),
        config.listen_host,
        config.port,
    )
    tcp_socket = tcp_server.sockets[0]
    actual_port = int(tcp_socket.getsockname()[1])
    loop = asyncio.get_running_loop()
    udp_transport, _ = await loop.create_datagram_endpoint(
        lambda: DatagramCaptureProtocol(store, pending_tasks),
        local_addr=(config.listen_host, actual_port),
    )
    runtime = ProbeRuntime(
        config=config,
        port=actual_port,
        store=store,
        tcp_server=tcp_server,
        udp_transport=udp_transport,
        pending_tasks=pending_tasks,
        tcp_tasks=tcp_tasks,
    )
    await store.record_event("ready", listen_host=config.listen_host, port=actual_port)
    return runtime


# //// /启动共享端口的 TCP 和 UDP 探针 ////


# //// 持续运行探针直到进程收到终止信号 [@x380kkm 2026-07-20] ////
async def run_probe(config: ProbeConfig) -> None:
    runtime = await start_probe(config)
    stopped = asyncio.Event()
    loop = asyncio.get_running_loop()

    def request_stop(*_: object) -> None:
        loop.call_soon_threadsafe(stopped.set)

    signal.signal(signal.SIGINT, request_stop)
    if hasattr(signal, "SIGTERM"):
        signal.signal(signal.SIGTERM, request_stop)
    try:
        await stopped.wait()
    finally:
        await runtime.close()


# //// /持续运行探针直到进程收到终止信号 ////


# //// 解析命令行参数 [@x380kkm 2026-07-20] ////
def parse_arguments() -> ProbeConfig:
    parser = argparse.ArgumentParser(description="记录多人端口收到的 TCP 和 UDP 客户端字节.")
    parser.add_argument("--listen-host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=8003)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    if not 0 <= arguments.port <= 65535:
        parser.error("--port 必须位于 0 到 65535.")
    return ProbeConfig(
        listen_host=arguments.listen_host,
        port=arguments.port,
        output_directory=arguments.output.resolve(),
    )


# //// /解析命令行参数 ////


# //// 启动命令行入口 [@x380kkm 2026-07-20] ////
def main() -> None:
    asyncio.run(run_probe(parse_arguments()))


if __name__ == "__main__":
    main()
# //// /启动命令行入口 ////
