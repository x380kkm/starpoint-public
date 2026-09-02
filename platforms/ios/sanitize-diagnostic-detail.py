# audience: internal
# # sanitize-ios-diagnostic-detail
# 此脚本从 stdin 读取诊断文本, 删除凭据形状, 并输出末尾的有限长度.

from __future__ import annotations

import re
import sys


# //// 删除诊断文本中的凭据形状并保留尾部 [@x380kkm 2026-08-18] ////
def sanitize_diagnostic_detail(text: str, limit: int = 800) -> str:
    if limit < 1:
        raise ValueError("limit must be positive")

    protected = re.sub(
        r"(?is)-----BEGIN (?P<label>[A-Z0-9 ]+)-----.*?(?:-----END (?P=label)-----|\Z)",
        "[redacted-pem]",
        text,
    )
    protected = re.sub(
        r"(?i)(https?://)[^/\s:@]+:[^@\s/]+@",
        r"\1[redacted]@",
        protected,
    )
    protected = re.sub(
        r"(?i)(authorization\s*[:=]\s*)(?:bearer|basic)\s+\S+",
        r"\1[redacted]",
        protected,
    )
    protected = re.sub(r"(?i)(\bbearer\s+)\S+", r"\1[redacted]", protected)
    credential_key = (
        r"(?:authorization|token|password|secret|private[_-]?key|api[_-]?key|"
        r"access[_-]?token|refresh[_-]?token|session[_-]?token)"
    )
    credential_prefix = rf'''(["']?{credential_key}["']?\s*[:=]\s*)'''
    protected = re.sub(
        "(?i)" + credential_prefix + r'"(?:\\.|[^"\\])*(?:"|\Z)',
        r'\1"[redacted]"',
        protected,
    )
    protected = re.sub(
        "(?i)" + credential_prefix + r"'(?:\\.|[^'\\])*(?:'|\Z)",
        r"\1'[redacted]'",
        protected,
    )
    protected = re.sub(
        "(?i)" + credential_prefix + r"[^,\s;}]+",
        r"\1[redacted]",
        protected,
    )
    protected = re.sub(r"[\r\n\t]+", " ", protected).strip()
    return protected[-limit:]


# //// /删除诊断文本中的凭据形状并保留尾部 ////


# //// 处理标准输入中的诊断文本 [@x380kkm 2026-08-18] ////
def main() -> int:
    sys.stdout.write(sanitize_diagnostic_detail(sys.stdin.read()))
    return 0


# //// /处理标准输入中的诊断文本 ////


# //// 执行命令行入口 [@x380kkm 2026-08-18] ////
if __name__ == "__main__":
    raise SystemExit(main())
# //// /执行命令行入口 ////
