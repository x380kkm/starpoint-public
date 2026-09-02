# audience: internal
# # test-protocol-pcap-analysis
# 此测试验证 PCAP 解析器保留 TCP 握手, 重组分段 JSON, 去除重传并区分连接方向.

from __future__ import annotations

import json
import socket
import struct
import tempfile
import unittest
from pathlib import Path

from analyze_pcap import analyze_capture


# //// 构造测试使用的 Ethernet IPv4 TCP 包 [@x380kkm 2026-07-21] ////
def build_tcp_packet(
    source_address: str,
    source_port: int,
    destination_address: str,
    destination_port: int,
    sequence: int,
    payload: bytes,
    flags: int = 0x18,
) -> bytes:
    tcp_header = struct.pack(
        "!HHIIBBHHH",
        source_port,
        destination_port,
        sequence,
        0,
        5 << 4,
        flags,
        65535,
        0,
        0,
    )
    total_length = 20 + len(tcp_header) + len(payload)
    ip_header = struct.pack(
        "!BBHHHBBH4s4s",
        0x45,
        0,
        total_length,
        1,
        0,
        64,
        6,
        0,
        socket.inet_aton(source_address),
        socket.inet_aton(destination_address),
    )
    ethernet_header = b"\x00" * 12 + struct.pack("!H", 0x0800)
    return ethernet_header + ip_header + tcp_header + payload


# //// /构造测试使用的 Ethernet IPv4 TCP 包 ////


# //// 写入包含指定包的经典 PCAP [@x380kkm 2026-07-21] ////
def write_pcap(path: Path, packets: list[bytes]) -> None:
    with path.open("wb") as stream:
        stream.write(b"\xd4\xc3\xb2\xa1")
        stream.write(struct.pack("<HHIIII", 2, 4, 0, 0, 65535, 1))
        for index, packet in enumerate(packets):
            stream.write(struct.pack("<IIII", 1_700_000_000 + index, index, len(packet), len(packet)))
            stream.write(packet)


# //// /写入包含指定包的经典 PCAP ////


# //// 验证握手、分段帧、合并帧和重传去重 [@x380kkm 2026-07-21] ////
class AnalyzePcapTest(unittest.TestCase):
    def test_reassembles_json_frames_and_removes_retransmission(self) -> None:
        client = ("10.0.2.15", 41000)
        server = ("10.0.2.2", 8003)
        handshake = b'{"socklet":"cooperation_battle","roomNumber":"654678"}'
        first = handshake[:23]
        second = handshake[23:] + b"\0[1,[3]]\0"
        packets = [
            build_tcp_packet(*client, *server, 100, first),
            build_tcp_packet(*client, *server, 100 + len(first), second),
            build_tcp_packet(*client, *server, 100 + len(first), second),
            build_tcp_packet(*server, *client, 500, b'[0,"654678",""]\0'),
        ]

        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "capture.pcap"
            write_pcap(path, packets)
            result = analyze_capture(path, 8003)

        self.assertEqual(1, result["connection_count"])
        directions = result["connections"][0]["directions"]
        client_direction = next(item for item in directions if item["source"]["port"] == 41000)
        server_direction = next(item for item in directions if item["source"]["port"] == 8003)
        self.assertEqual(3, client_direction["segment_count"])
        self.assertEqual(2, len(client_direction["frames"]))
        self.assertEqual("cooperation_battle", client_direction["frames"][0]["value"]["socklet"])
        self.assertEqual([1, [3]], client_direction["frames"][1]["value"])
        self.assertEqual([0, "654678", ""], server_direction["frames"][0]["value"])
        self.assertEqual([], client_direction["incomplete_blocks"])

    # //// 验证重组 HTTP 请求和响应时隐藏敏感载荷 [@x380kkm 2026-07-27] ////
    def test_records_safe_http_messages_after_reassembly(self) -> None:
        client = ("10.0.2.15", 41002)
        server = ("10.0.2.2", 8003)
        request_body = b'{"token":"secret"}\0'
        request = (
            b"POST /sessions?token=secret&viewer_id=123 HTTP/1.1\r\n"
            b"Host: private.example\r\n"
            b"Authorization: Bearer header-secret\r\n"
            b"Content-Type: application/json\r\n"
            + f"Content-Length: {len(request_body)}\r\n\r\n".encode()
            + request_body
        )
        response_body = b'{"ok":true}'
        response = (
            b"HTTP/1.1 200 OK\r\n"
            b"Content-Type: application/json\r\n"
            b"Content-Length: 11\r\n\r\n"
            + response_body
        )
        request_split = len(request) - len(request_body) + 5
        response_split = 29
        packets = [
            build_tcp_packet(*client, *server, 100, request[:request_split]),
            build_tcp_packet(*client, *server, 100 + request_split, request[request_split:]),
            build_tcp_packet(*server, *client, 500, response[:response_split]),
            build_tcp_packet(*server, *client, 500 + response_split, response[response_split:]),
        ]

        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "capture.pcap"
            write_pcap(path, packets)
            result = analyze_capture(path, 8003)

        directions = result["connections"][0]["directions"]
        client_direction = next(item for item in directions if item["source"]["port"] == 41002)
        server_direction = next(item for item in directions if item["source"]["port"] == 8003)
        self.assertEqual(
            {
                "method": "POST",
                "path": "/sessions",
                "content_type": "application/json",
                "content_length": len(request_body),
                "body_complete": True,
            },
            {
                key: client_direction["http_messages"][0][key]
                for key in ("method", "path", "content_type", "content_length", "body_complete")
            },
        )
        self.assertEqual(
            {
                "status": 200,
                "content_type": "application/json",
                "content_length": len(response_body),
                "body_complete": True,
            },
            {
                key: server_direction["http_messages"][0][key]
                for key in ("status", "content_type", "content_length", "body_complete")
            },
        )
        serialized_directions = json.dumps([client_direction, server_direction])
        self.assertNotIn("private.example", serialized_directions)
        self.assertNotIn("header-secret", serialized_directions)
        self.assertNotIn("secret", serialized_directions)
        self.assertEqual([], client_direction["frames"])
        self.assertEqual([], server_direction["frames"])
        self.assertEqual([], client_direction["incomplete_blocks"])
        self.assertEqual([], server_direction["incomplete_blocks"])
        self.assertTrue(
            all(
                {"host", "body"}.isdisjoint(message)
                for direction in (client_direction, server_direction)
                for message in direction["http_messages"]
            )
        )

    # //// /验证重组 HTTP 请求和响应时隐藏敏感载荷 ////

    # //// 验证截断 HTTP 头不输出原始前缀 [@x380kkm 2026-07-27] ////
    def test_redacts_truncated_http_header(self) -> None:
        client = ("10.0.2.15", 41004)
        server = ("10.0.2.2", 8003)
        truncated_request = (
            b"POST /sessions?token=secret HTTP/1.1\r\n"
            b"Authorization: Bearer header-secret"
        )
        packets = [build_tcp_packet(*client, *server, 100, truncated_request)]

        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "capture.pcap"
            write_pcap(path, packets)
            result = analyze_capture(path, 8003)

        direction = result["connections"][0]["directions"][0]
        self.assertEqual([], direction["http_messages"])
        self.assertEqual([], direction["frames"])
        self.assertNotIn("payload_prefix_hex", direction["incomplete_blocks"][0])
        self.assertIn("payload_sha256", direction["incomplete_blocks"][0])
        self.assertNotIn("secret", json.dumps(direction))

    # //// /验证截断 HTTP 头不输出原始前缀 ////

    # //// 验证完整 HTTP 后继续解码同块 JSON 帧 [@x380kkm 2026-07-27] ////
    def test_preserves_json_frames_after_complete_http_message(self) -> None:
        client = ("10.0.2.15", 41003)
        server = ("10.0.2.2", 8003)
        request = b"POST /metadata HTTP/1.1\r\nContent-Length: 0\r\n\r\n"
        json_frame = b'[1,["next"]]\0'
        packets = [build_tcp_packet(*client, *server, 100, request + json_frame)]

        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "capture.pcap"
            write_pcap(path, packets)
            result = analyze_capture(path, 8003)

        direction = result["connections"][0]["directions"][0]
        self.assertEqual("/metadata", direction["http_messages"][0]["path"])
        self.assertEqual([1, ["next"]], direction["frames"][0]["value"])
        self.assertEqual([], direction["incomplete_blocks"])

    # //// /验证完整 HTTP 后继续解码同块 JSON 帧 ////

    def test_records_handshake_without_payload(self) -> None:
        client = ("10.0.2.15", 41001)
        server = ("10.0.2.2", 8003)
        packets = [
            build_tcp_packet(*client, *server, 100, b"", flags=0x02),
            build_tcp_packet(*server, *client, 500, b"", flags=0x12),
            build_tcp_packet(*client, *server, 101, b"", flags=0x10),
        ]

        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "capture.pcap"
            write_pcap(path, packets)
            result = analyze_capture(path, 8003)

        self.assertEqual(3, result["selected_segment_count"])
        self.assertEqual(1, result["connection_count"])
        directions = result["connections"][0]["directions"]
        client_direction = next(item for item in directions if item["source"]["port"] == 41001)
        server_direction = next(item for item in directions if item["source"]["port"] == 8003)
        self.assertEqual(0, client_direction["payload_segment_count"])
        self.assertEqual({"ACK": 1, "SYN": 1}, client_direction["tcp_flag_counts"])
        self.assertEqual({"SYN+ACK": 1}, server_direction["tcp_flag_counts"])
        self.assertEqual([], client_direction["frames"])

    def test_rejects_non_pcap_input(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "capture.pcap"
            path.write_text(json.dumps({"not": "pcap"}), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "经典 PCAP"):
                analyze_capture(path, 8003)


# //// /验证握手、分段帧、合并帧和重传去重 ////


if __name__ == "__main__":
    unittest.main()
