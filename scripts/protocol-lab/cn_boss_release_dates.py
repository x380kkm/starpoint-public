# audience: internal
# # cn-boss-release-dates
# 公告时间携带日服 UTC+9, 主表日期按国服 UTC+8 序列化为同一时刻.
# 来源表按关卡 ID 保存原始日期, 同名领主的活动与重发记录分别关联.

import importlib.util
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path


SOURCE_PATH = Path(__file__).with_name("cn-boss-release-sources.json")
CN_TIMEZONE = timezone(timedelta(hours=8))


# //// 读取官方超级公告对应的关卡日期 [@x380kkm 2026-09-26] ////
def boss_release_dates() -> dict[int, dict]:
    sources = json.loads(SOURCE_PATH.read_text(encoding="utf-8"))
    return {entry["quest_id"]: entry for entry in sources["quests"]
            if entry["schedule_basis"] == "official_jp_super_release"}


# //// 将带时区的日服公告时间转换为国服主表日期 [@x380kkm 2026-09-26] ////
def cn_release_time(release: dict) -> str:
    moment = datetime.fromisoformat(release["jp_super_release_at"])
    return moment.astimezone(CN_TIMEZONE).strftime("%Y-%m-%d %H:%M:%S")


# //// 按原始日期匹配公告并保留独立重发窗口 [@x380kkm 2026-09-26] ////
def apply_boss_row_release(row: list[str], releases: dict[int, dict]) -> None:
    release = releases.get(int(row[0]))
    if release is None:
        return
    target = cn_release_time(release)
    if row[5] not in (release["master_start_at"], target):
        raise ValueError(f"领主公告与主表窗口不匹配: {row[0]} {row[5]}")
    row[5] = target


# //// 复用多行主表编解码边界 [@x380kkm 2026-09-26] ////
def ordered_map_codec():
    name = "build_cn_voice_overlay"
    if name in sys.modules:
        return sys.modules[name]
    path = Path(__file__).with_name("build-cn-voice-overlay.py")
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


# //// 修正领主主表时间并保留关卡结构和列值 [@x380kkm 2026-09-26] ////
def apply_boss_master_releases(content: bytes) -> bytes:
    codec = ordered_map_codec()
    master = codec.decode_ordered_map(content)
    releases = boss_release_dates()

    def visit(value):
        if isinstance(value, dict):
            for child in value.values():
                visit(child)
        else:
            for row in value:
                apply_boss_row_release(row, releases)

    visit(master)
    return codec.encode_ordered_map(master)
