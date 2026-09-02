# audience: external
# # protocol-pcap-analysis
# 此程序从经典 PCAP 中提取指定端口的 IPv4 TCP 连接, 去除负载重传并重组 NUL 结尾的 JSON 帧和 HTTP 元数据.
# 此程序只读取捕获文件, 不启动网络服务, 输出可重复生成的 JSON 分析结果.

from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import struct
from collections import defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, BinaryIO, Iterator
from urllib.parse import urlsplit

from multiplayer_probe import NulJsonFrameDecoder


# //// 表示一个 TCP 端点 [@x380kkm 2026-07-21] ////
@dataclass(frozen=True, order=True)
class Endpoint:
    address: str
    port: int


# //// /表示一个 TCP 端点 ////


# //// 表示 PCAP 中一个 TCP 段 [@x380kkm 2026-07-21] ////
@dataclass(frozen=True)
class TcpSegment:
    timestamp: float
    source: Endpoint
    destination: Endpoint
    sequence: int
    flags: int
    payload: bytes


# //// /表示 PCAP 中一个 TCP 段 ////


# //// 表示重组后保持来源时间的连续字节块 [@x380kkm 2026-07-21] ////
@dataclass(frozen=True)
class ReassembledChunk:
    timestamp: float
    payload: bytes


# //// /表示重组后保持来源时间的连续字节块 ////


_PCAP_MAGIC = {
    b"\xd4\xc3\xb2\xa1": ("<", 1_000_000),
    b"\xa1\xb2\xc3\xd4": (">", 1_000_000),
    b"\x4d\x3c\xb2\xa1": ("<", 1_000_000_000),
    b"\xa1\xb2\x3c\x4d": (">", 1_000_000_000),
}
_HTTP_METHODS = {
    b"CONNECT",
    b"DELETE",
    b"GET",
    b"HEAD",
    b"OPTIONS",
    b"PATCH",
    b"POST",
    b"PUT",
    b"TRACE",
}


# //// 从文件读取指定长度并拒绝截断数据 [@x380kkm 2026-07-21] ////
def _read_exact(stream: BinaryIO, length: int) -> bytes:
    payload = stream.read(length)
    if len(payload) != length:
        raise ValueError(f"PCAP 数据截断, 需要 {length} 字节, 实际 {len(payload)} 字节.")
    return payload


# //// /从文件读取指定长度并拒绝截断数据 ////


# //// 从经典 PCAP 逐包返回时间和链路层数据 [@x380kkm 2026-07-21] ////
def _iter_pcap_packets(path: Path) -> Iterator[tuple[float, bytes]]:
    with path.open("rb") as stream:
        magic = _read_exact(stream, 4)
        format_info = _PCAP_MAGIC.get(magic)
        if format_info is None:
            raise ValueError("仅支持经典 PCAP, 不支持当前文件格式.")

        byte_order, timestamp_scale = format_info
        version_major, version_minor, _, _, _, link_type = struct.unpack(
            f"{byte_order}HHIIII", _read_exact(stream, 20)
        )
        if (version_major, version_minor) != (2, 4):
            raise ValueError(f"不支持 PCAP 版本 {version_major}.{version_minor}.")
        if link_type != 1:
            raise ValueError(f"仅支持 Ethernet 链路类型, 当前类型为 {link_type}.")

        packet_header = struct.Struct(f"{byte_order}IIII")
        while header := stream.read(packet_header.size):
            if len(header) != packet_header.size:
                raise ValueError("PCAP 包头截断.")
            seconds, fraction, captured_length, _ = packet_header.unpack(header)
            packet = _read_exact(stream, captured_length)
            yield seconds + fraction / timestamp_scale, packet


# //// /从经典 PCAP 逐包返回时间和链路层数据 ////


# //// 从 Ethernet IPv4 包提取一个 TCP 段 [@x380kkm 2026-07-21] ////
def _parse_tcp_segment(timestamp: float, packet: bytes) -> TcpSegment | None:
    if len(packet) < 14:
        return None

    ethernet_offset = 14
    ether_type = struct.unpack_from("!H", packet, 12)[0]
    while ether_type in (0x8100, 0x88A8):
        if len(packet) < ethernet_offset + 4:
            return None
        ether_type = struct.unpack_from("!H", packet, ethernet_offset + 2)[0]
        ethernet_offset += 4
    if ether_type != 0x0800 or len(packet) < ethernet_offset + 20:
        return None

    version_and_header = packet[ethernet_offset]
    if version_and_header >> 4 != 4:
        return None
    ip_header_length = (version_and_header & 0x0F) * 4
    if ip_header_length < 20 or len(packet) < ethernet_offset + ip_header_length:
        return None
    total_length = struct.unpack_from("!H", packet, ethernet_offset + 2)[0]
    ip_end = min(len(packet), ethernet_offset + total_length)
    fragment = struct.unpack_from("!H", packet, ethernet_offset + 6)[0]
    if fragment & 0x3FFF or packet[ethernet_offset + 9] != 6:
        return None

    tcp_offset = ethernet_offset + ip_header_length
    if ip_end < tcp_offset + 20:
        return None
    source_port, destination_port, sequence = struct.unpack_from("!HHI", packet, tcp_offset)
    tcp_header_length = (packet[tcp_offset + 12] >> 4) * 4
    if tcp_header_length < 20 or ip_end < tcp_offset + tcp_header_length:
        return None
    flags = packet[tcp_offset + 13]
    payload = packet[tcp_offset + tcp_header_length : ip_end]

    source_address = str(ipaddress.ip_address(packet[ethernet_offset + 12 : ethernet_offset + 16]))
    destination_address = str(ipaddress.ip_address(packet[ethernet_offset + 16 : ethernet_offset + 20]))
    return TcpSegment(
        timestamp=timestamp,
        source=Endpoint(source_address, source_port),
        destination=Endpoint(destination_address, destination_port),
        sequence=sequence,
        flags=flags,
        payload=payload,
    )


# //// /从 Ethernet IPv4 包提取一个 TCP 段 ////


# //// 把一个方向的 TCP 段去重并拆成无缺口字节块 [@x380kkm 2026-07-21] ////
def _reassemble_segments(segments: list[TcpSegment]) -> list[list[ReassembledChunk]]:
    blocks: list[list[ReassembledChunk]] = []
    current_block: list[ReassembledChunk] = []
    next_sequence: int | None = None

    for segment in sorted(segments, key=lambda item: (item.sequence, item.timestamp)):
        segment_end = segment.sequence + len(segment.payload)
        if next_sequence is None or segment.sequence > next_sequence:
            if current_block:
                blocks.append(current_block)
            current_block = []
            next_sequence = segment.sequence
        if segment_end <= next_sequence:
            continue

        overlap = max(0, next_sequence - segment.sequence)
        payload = segment.payload[overlap:]
        current_block.append(ReassembledChunk(segment.timestamp, payload))
        next_sequence += len(payload)

    if current_block:
        blocks.append(current_block)
    return blocks


# //// /把一个方向的 TCP 段去重并拆成无缺口字节块 ////


# //// 把 Unix 时间转换为稳定的 UTC 文本 [@x380kkm 2026-07-21] ////
def _format_timestamp(timestamp: float) -> str:
    return datetime.fromtimestamp(timestamp, UTC).isoformat()


# //// /把 Unix 时间转换为稳定的 UTC 文本 ////


# //// 为可能含 HTTP 凭据的负载生成脱敏摘要 [@x380kkm 2026-07-27] ////
def _redacted_payload_evidence(payload: bytes) -> dict[str, str]:
    method, separator, _ = payload.partition(b" ")
    is_http_payload = payload.startswith(b"HTTP/") or (separator != b"" and method in _HTTP_METHODS)
    evidence = {"payload_sha256": hashlib.sha256(payload).hexdigest()}
    if not is_http_payload:
        evidence["payload_prefix_hex"] = payload[:64].hex()
    return evidence


# //// /为可能含 HTTP 凭据的负载生成脱敏摘要 ////


# //// 将 TCP 标志位转换为稳定名称 [@x380kkm 2026-07-21] ////
def _format_tcp_flags(flags: int) -> str:
    names = [
        name
        for mask, name in (
            (0x01, "FIN"),
            (0x02, "SYN"),
            (0x04, "RST"),
            (0x08, "PSH"),
            (0x10, "ACK"),
            (0x20, "URG"),
            (0x40, "ECE"),
            (0x80, "CWR"),
        )
        if flags & mask
    ]
    return "+".join(names) if names else "NONE"


# //// /将 TCP 标志位转换为稳定名称 ////


# //// 从完整 HTTP 头提取不含敏感值的消息元数据 [@x380kkm 2026-07-27] ////
def _parse_http_metadata(header: bytes) -> dict[str, Any] | None:
    try:
        lines = header.decode("iso-8859-1").split("\r\n")
    except UnicodeDecodeError:
        return None
    if not lines:
        return None

    start_line = lines[0].split(" ")
    headers: dict[str, str] = {}
    for line in lines[1:]:
        name, separator, value = line.partition(":")
        if separator:
            headers[name.lower()] = value.strip()

    content_length: int | None = None
    raw_content_length = headers.get("content-length")
    if raw_content_length is not None:
        try:
            parsed_content_length = int(raw_content_length)
        except ValueError:
            parsed_content_length = -1
        if parsed_content_length >= 0:
            content_length = parsed_content_length

    metadata: dict[str, Any]
    if len(start_line) >= 2 and start_line[0].startswith("HTTP/"):
        try:
            status = int(start_line[1])
        except ValueError:
            return None
        metadata = {"kind": "response", "http_version": start_line[0], "status": status}
    elif len(start_line) >= 3 and start_line[0].isalpha() and start_line[2].startswith("HTTP/"):
        method = start_line[0]
        metadata = {
            "kind": "request",
            "method": method,
            "http_version": start_line[2],
        }
        if method != "CONNECT":
            try:
                metadata["path"] = urlsplit(start_line[1]).path or "/"
            except ValueError:
                return None
    else:
        return None

    content_type = headers.get("content-type")
    if content_type is not None:
        metadata["content_type"] = content_type.split(";", 1)[0].strip()
    if content_length is not None:
        metadata["content_length"] = content_length
    return metadata


# //// /从完整 HTTP 头提取不含敏感值的消息元数据 ////
# //// 从连续 TCP 字节块提取 HTTP 元数据和安全跳过长度 [@x380kkm 2026-07-27] ////
def _decode_http_messages(
    block: list[ReassembledChunk], block_index: int
) -> tuple[list[dict[str, Any]], int]:
    payload = b"".join(chunk.payload for chunk in block)
    messages: list[dict[str, Any]] = []
    offset = 0

    while offset < len(payload):
        header_end = payload.find(b"\r\n\r\n", offset)
        if header_end < 0:
            break
        metadata = _parse_http_metadata(payload[offset:header_end])
        if metadata is None:
            break

        header_end += 4
        content_length = metadata.get("content_length")
        body_end = header_end if content_length is None else header_end + content_length
        metadata["timestamp"] = _format_timestamp(block[0].timestamp)
        metadata["block"] = block_index
        metadata["body_complete"] = None if content_length is None else body_end <= len(payload)
        messages.append(metadata)
        if content_length is None or body_end > len(payload):
            return messages, len(payload)
        offset = body_end

    return messages, offset


# //// /从连续 TCP 字节块提取 HTTP 元数据和安全跳过长度 ////
# //// 解码一个重组块中的 NUL JSON 帧 [@x380kkm 2026-07-27] ////
def _decode_json_frames(
    block: list[ReassembledChunk], block_index: int
) -> tuple[list[dict[str, Any]], dict[str, Any] | None]:
    decoder = NulJsonFrameDecoder()
    frames: list[dict[str, Any]] = []
    for chunk in block:
        for frame in decoder.feed(chunk.payload):
            item: dict[str, Any] = {
                "timestamp": _format_timestamp(chunk.timestamp),
                "block": block_index,
                "payload_bytes": len(frame.payload),
                "value": frame.value,
                "error": frame.error,
            }
            if frame.error is not None:
                item.update(_redacted_payload_evidence(frame.payload))
            frames.append(item)

    remainder = decoder.remainder()
    if not remainder:
        return frames, None
    incomplete_block: dict[str, Any] = {
        "block": block_index,
        "payload_bytes": len(remainder),
    }
    incomplete_block.update(_redacted_payload_evidence(remainder))
    return frames, incomplete_block


# //// /解码一个重组块中的 NUL JSON 帧 ////
# //// 解码一个 TCP 方向内的全部连续 JSON 帧 [@x380kkm 2026-07-21] ////
def _decode_direction(segments: list[TcpSegment]) -> dict[str, Any]:
    frames: list[dict[str, Any]] = []
    http_messages: list[dict[str, Any]] = []
    incomplete_blocks: list[dict[str, Any]] = []
    reassembled_bytes = 0
    payload_segments = [segment for segment in segments if segment.payload]
    flag_counts: dict[str, int] = defaultdict(int)
    for segment in segments:
        flag_counts[_format_tcp_flags(segment.flags)] += 1

    for block_index, block in enumerate(_reassemble_segments(payload_segments)):
        block_http_messages, http_payload_bytes = _decode_http_messages(block, block_index)
        http_messages.extend(block_http_messages)
        reassembled_bytes += sum(len(chunk.payload) for chunk in block)
        if block_http_messages:
            remaining_payload = b"".join(chunk.payload for chunk in block)[http_payload_bytes:]
            if not remaining_payload:
                continue
            json_block = [ReassembledChunk(block[0].timestamp, remaining_payload)]
        else:
            json_block = block
        block_frames, incomplete_block = _decode_json_frames(json_block, block_index)
        frames.extend(block_frames)
        if incomplete_block is not None:
            incomplete_blocks.append(incomplete_block)

    return {
        "segment_count": len(segments),
        "payload_segment_count": len(payload_segments),
        "captured_payload_bytes": sum(len(segment.payload) for segment in segments),
        "reassembled_payload_bytes": reassembled_bytes,
        "tcp_flag_counts": dict(sorted(flag_counts.items())),
        "frames": frames,
        "http_messages": http_messages,
        "incomplete_blocks": incomplete_blocks,
    }


# //// /解码一个 TCP 方向内的全部连续 JSON 帧 ////


# //// 分析指定端口并返回可序列化的连接结果 [@x380kkm 2026-07-21] ////
def analyze_capture(path: Path, port: int) -> dict[str, Any]:
    if not 1 <= port <= 65535:
        raise ValueError("端口必须处于 1 到 65535.")

    directions: dict[tuple[Endpoint, Endpoint], list[TcpSegment]] = defaultdict(list)
    packet_count = 0
    selected_segment_count = 0
    for timestamp, packet in _iter_pcap_packets(path):
        packet_count += 1
        segment = _parse_tcp_segment(timestamp, packet)
        if segment is None or port not in (segment.source.port, segment.destination.port):
            continue
        directions[(segment.source, segment.destination)].append(segment)
        selected_segment_count += 1

    connection_directions: dict[
        tuple[Endpoint, Endpoint], list[tuple[Endpoint, Endpoint, list[TcpSegment]]]
    ] = defaultdict(list)
    for (source, destination), segments in directions.items():
        connection = tuple(sorted((source, destination)))
        connection_directions[connection].append((source, destination, segments))

    connections: list[dict[str, Any]] = []
    for endpoints, items in sorted(connection_directions.items()):
        serialized_directions: list[dict[str, Any]] = []
        for source, destination, segments in sorted(items, key=lambda item: (item[0], item[1])):
            direction = _decode_direction(segments)
            direction["source"] = {"address": source.address, "port": source.port}
            direction["destination"] = {"address": destination.address, "port": destination.port}
            serialized_directions.append(direction)
        connections.append(
            {
                "endpoints": [
                    {"address": endpoint.address, "port": endpoint.port} for endpoint in endpoints
                ],
                "directions": serialized_directions,
            }
        )

    return {
        "pcap_path": str(path.resolve()),
        "port": port,
        "packet_count": packet_count,
        "selected_segment_count": selected_segment_count,
        "connection_count": len(connections),
        "connections": connections,
    }


# //// /分析指定端口并返回可序列化的连接结果 ////


# //// 解析命令行参数 [@x380kkm 2026-07-21] ////
def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="解析 PCAP 中的多人 TCP JSON 帧和 HTTP 元数据.")
    parser.add_argument("pcap", type=Path, help="经典 PCAP 文件路径.")
    parser.add_argument("--port", type=int, default=8003, help="需要分析的 TCP 端口.")
    parser.add_argument("--output", type=Path, help="JSON 输出文件. 省略时写到 stdout.")
    return parser.parse_args()


# //// /解析命令行参数 ////


# //// 执行 PCAP 分析并输出 JSON [@x380kkm 2026-07-21] ////
def main() -> None:
    arguments = parse_arguments()
    result = analyze_capture(arguments.pcap, arguments.port)
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if arguments.output is None:
        print(rendered, end="")
        return
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(rendered, encoding="utf-8")


# //// /执行 PCAP 分析并输出 JSON ////


if __name__ == "__main__":
    main()
