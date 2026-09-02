# audience: internal
# # ios-cn-messagepack
# 该模块提供 Simulator 协议诊断需要的标准 MessagePack 编解码, 并只依赖 Python 标准库.

import math
import struct


# //// 写入标准 MessagePack 长度前缀 [@x380kkm 2026-08-21] ////
def _pack_length(output, length, fixed_prefix, fixed_limit, prefix8, prefix16, prefix32):
    if length < fixed_limit:
        output.append(fixed_prefix | length)
    elif length <= 0xFF and prefix8 is not None:
        output.extend(struct.pack(">BB", prefix8, length))
    elif length <= 0xFFFF:
        output.append(prefix16)
        output.extend(struct.pack(">H", length))
    elif length <= 0xFFFFFFFF:
        output.append(prefix32)
        output.extend(struct.pack(">I", length))
    else:
        raise OverflowError("MessagePack value is too large")


# //// 写入标准 MessagePack 整数 [@x380kkm 2026-08-21] ////
def _pack_integer(output, value):
    if value >= 0:
        if value <= 0x7F:
            output.append(value)
        elif value <= 0xFF:
            output.extend(struct.pack(">BB", 0xCC, value))
        elif value <= 0xFFFF:
            output.append(0xCD)
            output.extend(struct.pack(">H", value))
        elif value <= 0xFFFFFFFF:
            output.append(0xCE)
            output.extend(struct.pack(">I", value))
        elif value <= 0xFFFFFFFFFFFFFFFF:
            output.append(0xCF)
            output.extend(struct.pack(">Q", value))
        else:
            raise OverflowError("MessagePack integer is too large")
        return
    if value >= -32:
        output.append(0x100 + value)
    elif value >= -0x80:
        output.append(0xD0)
        output.extend(struct.pack(">b", value))
    elif value >= -0x8000:
        output.append(0xD1)
        output.extend(struct.pack(">h", value))
    elif value >= -0x80000000:
        output.append(0xD2)
        output.extend(struct.pack(">i", value))
    elif value >= -0x8000000000000000:
        output.append(0xD3)
        output.extend(struct.pack(">q", value))
    else:
        raise OverflowError("MessagePack integer is too small")


# //// 写入标准 MessagePack 根值 [@x380kkm 2026-08-21] ////
def _pack_value(output, value):
    if value is None:
        output.append(0xC0)
    elif value is False:
        output.append(0xC2)
    elif value is True:
        output.append(0xC3)
    elif isinstance(value, int):
        _pack_integer(output, value)
    elif isinstance(value, float) and math.isfinite(value):
        output.append(0xCB)
        output.extend(struct.pack(">d", value))
    elif isinstance(value, str):
        encoded = value.encode("utf-8")
        _pack_length(output, len(encoded), 0xA0, 32, 0xD9, 0xDA, 0xDB)
        output.extend(encoded)
    elif isinstance(value, (bytes, bytearray, memoryview)):
        encoded = bytes(value)
        _pack_length(output, len(encoded), 0, 0, 0xC4, 0xC5, 0xC6)
        output.extend(encoded)
    elif isinstance(value, (list, tuple)):
        _pack_length(output, len(value), 0x90, 16, None, 0xDC, 0xDD)
        for entry in value:
            _pack_value(output, entry)
    elif isinstance(value, dict):
        _pack_length(output, len(value), 0x80, 16, None, 0xDE, 0xDF)
        for key, entry in value.items():
            _pack_value(output, str(key))
            _pack_value(output, entry)
    else:
        raise TypeError("unsupported MessagePack value: %s" % type(value).__name__)


# //// 编码一个 MessagePack 根值 [@x380kkm 2026-08-21] ////
def pack_messagepack(value):
    output = bytearray()
    _pack_value(output, value)
    return bytes(output)


# //// 读取标准 MessagePack 数据 [@x380kkm 2026-08-21] ////
class _MessagePackDecoder:
    def __init__(self, value):
        self.buffer = bytes(value)
        self.offset = 0

    def _read_bytes(self, length):
        end = self.offset + length
        if end > len(self.buffer):
            raise ValueError("truncated MessagePack value")
        value = self.buffer[self.offset:end]
        self.offset = end
        return value

    def _read_number(self, format_string):
        length = struct.calcsize(format_string)
        return struct.unpack(format_string, self._read_bytes(length))[0]

    def _read_string(self, length):
        return self._read_bytes(length).decode("utf-8")

    def _read_array(self, length):
        return [self.read() for _ in range(length)]

    def _read_map(self, length):
        value = {}
        for _ in range(length):
            key = self.read()
            entry = self.read()
            value[str(key)] = entry
        return value

    def read(self):
        prefix = self._read_number(">B")
        if prefix <= 0x7F:
            return prefix
        if prefix >= 0xE0:
            return prefix - 0x100
        if prefix & 0xF0 == 0x80:
            return self._read_map(prefix & 0x0F)
        if prefix & 0xF0 == 0x90:
            return self._read_array(prefix & 0x0F)
        if prefix & 0xE0 == 0xA0:
            return self._read_string(prefix & 0x1F)
        if prefix == 0xC0:
            return None
        if prefix == 0xC2:
            return False
        if prefix == 0xC3:
            return True
        if prefix == 0xC4:
            return self._read_bytes(self._read_number(">B"))
        if prefix == 0xC5:
            return self._read_bytes(self._read_number(">H"))
        if prefix == 0xC6:
            return self._read_bytes(self._read_number(">I"))
        if prefix == 0xCA:
            return self._read_number(">f")
        if prefix == 0xCB:
            return self._read_number(">d")
        if prefix == 0xCC:
            return self._read_number(">B")
        if prefix == 0xCD:
            return self._read_number(">H")
        if prefix == 0xCE:
            return self._read_number(">I")
        if prefix == 0xCF:
            return self._read_number(">Q")
        if prefix == 0xD0:
            return self._read_number(">b")
        if prefix == 0xD1:
            return self._read_number(">h")
        if prefix == 0xD2:
            return self._read_number(">i")
        if prefix == 0xD3:
            return self._read_number(">q")
        if prefix == 0xD9:
            return self._read_string(self._read_number(">B"))
        if prefix == 0xDA:
            return self._read_string(self._read_number(">H"))
        if prefix == 0xDB:
            return self._read_string(self._read_number(">I"))
        if prefix == 0xDC:
            return self._read_array(self._read_number(">H"))
        if prefix == 0xDD:
            return self._read_array(self._read_number(">I"))
        if prefix == 0xDE:
            return self._read_map(self._read_number(">H"))
        if prefix == 0xDF:
            return self._read_map(self._read_number(">I"))
        raise TypeError("unsupported MessagePack prefix: 0x%02x" % prefix)


# //// 解码一个完整 MessagePack 根值 [@x380kkm 2026-08-21] ////
def unpack_messagepack(value):
    decoder = _MessagePackDecoder(value)
    result = decoder.read()
    if decoder.offset != len(decoder.buffer):
        raise ValueError("trailing MessagePack data")
    return result
