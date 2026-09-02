# audience: internal
# # test-multiplayer-probe
# 此测试验证探针保存 TCP 和 UDP 原始字节, 并验证 NUL JSON 帧重组与握手字段检查.

from __future__ import annotations

import asyncio
import json
import socket
import tempfile
import unittest
from pathlib import Path

from multiplayer_probe import (
    NulJsonFrameDecoder,
    ProbeConfig,
    ProbeRuntime,
    assess_handshake,
    start_probe,
)


# //// 验证 TCP 和 UDP 捕获保持输入字节不变 [@x380kkm 2026-07-20] ////
class MultiplayerProbeTest(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.output_directory = Path(self.temporary_directory.name)
        self.runtime: ProbeRuntime = await start_probe(
            ProbeConfig(
                listen_host="127.0.0.1",
                port=0,
                output_directory=self.output_directory,
            )
        )

    async def asyncTearDown(self) -> None:
        await self.runtime.close()
        self.temporary_directory.cleanup()

    async def test_records_tcp_and_udp_payloads(self) -> None:
        tcp_payload = b"\x00\x01starpoint-tcp\xff"
        udp_payload = b"\xfe\x02starpoint-udp\x00"

        _, writer = await asyncio.open_connection("127.0.0.1", self.runtime.port)
        writer.write(tcp_payload)
        await writer.drain()
        writer.close()
        await writer.wait_closed()

        udp_socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        udp_socket.setblocking(False)
        try:
            await asyncio.get_running_loop().sock_sendto(
                udp_socket,
                udp_payload,
                ("127.0.0.1", self.runtime.port),
            )
        finally:
            udp_socket.close()

        payload_events = await self._wait_for_payload_events(2)
        saved_payloads = {
            event["transport"]: Path(event["payload_path"]).read_bytes()
            for event in payload_events
        }
        self.assertEqual(tcp_payload, saved_payloads["tcp"])
        self.assertEqual(udp_payload, saved_payloads["udp"])

    async def test_decodes_split_and_coalesced_handshake_frames(self) -> None:
        room_handshake = {
            "reconnected": 0,
            "socklet": "cooperation_room",
            "viewerId": 123,
            "roomNumber": "123456",
            "questCategory": 1,
            "questId": 1001,
        }
        battle_handshake = {
            "reconnected": 0,
            "socklet": "cooperation_battle",
            "connectionId": "connection-1",
            "roomNumber": "123456",
        }
        room_frame = json.dumps(room_handshake, separators=(",", ":")).encode() + b"\0"
        battle_frame = json.dumps(battle_handshake, separators=(",", ":")).encode() + b"\0"

        _, writer = await asyncio.open_connection("127.0.0.1", self.runtime.port)
        writer.write(room_frame[:17])
        await writer.drain()
        writer.write(room_frame[17:] + battle_frame)
        await writer.drain()
        writer.close()
        await writer.wait_closed()

        frame_events = await self._wait_for_events("tcp_json_frame", 2)
        self.assertEqual(room_handshake, frame_events[0]["message"])
        self.assertEqual(battle_handshake, frame_events[1]["message"])
        self.assertTrue(frame_events[0]["handshake"]["valid"])
        self.assertTrue(frame_events[1]["handshake"]["valid"])

    def test_decoder_preserves_incomplete_data_and_reports_invalid_json(self) -> None:
        decoder = NulJsonFrameDecoder()

        self.assertEqual([], decoder.feed(b'{"socklet":"cooperation_room"'))
        frames = decoder.feed(b"}\0not-json\0tail")

        self.assertEqual("cooperation_room", frames[0].value["socklet"])
        self.assertIsNone(frames[0].error)
        self.assertIsNone(frames[1].value)
        self.assertIsNotNone(frames[1].error)
        self.assertEqual(b"tail", decoder.remainder())

    def test_handshake_assessment_lists_missing_fields(self) -> None:
        assessment = assess_handshake(
            {
                "reconnected": 0,
                "socklet": "cooperation_battle",
                "roomNumber": "123456",
            }
        )

        self.assertEqual(["connectionId"], assessment["missing_fields"])
        self.assertFalse(assessment["valid"])

    def test_decoder_rejects_an_unbounded_incomplete_frame(self) -> None:
        decoder = NulJsonFrameDecoder(max_frame_bytes=8)

        with self.assertRaisesRegex(ValueError, "字节上限"):
            decoder.feed(b"123456789")

    async def _wait_for_payload_events(self, count: int) -> list[dict[str, object]]:
        return await self._wait_for_events("payload", count)

    async def _wait_for_events(self, event_name: str, count: int) -> list[dict[str, object]]:
        event_path = self.output_directory / "events.jsonl"
        for _ in range(100):
            events = [json.loads(line) for line in event_path.read_text(encoding="utf-8").splitlines()]
            matching_events = [event for event in events if event["event"] == event_name]
            if len(matching_events) >= count:
                return matching_events
            await asyncio.sleep(0.02)
        self.fail(f"未在时限内记录 {count} 个 {event_name} 事件.")


# //// /验证 TCP 和 UDP 捕获保持输入字节不变 ////


if __name__ == "__main__":
    unittest.main()
