# audience: internal
# # prepare-cn-announcement-replacements
# 此脚本将官方宣传图和缺图提示排入客户端公告的 1440x1920 卡面.
# 官方图片保留完整画面与署名, 来源和语言随资源准备记录发布.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


WIDTH, HEIGHT = 1440, 1920
ANNOUNCEMENTS = (
    (2064, "new_character_pickup_scollabo_01", ("新角色特选扭蛋",)),
    (3026, "special_pack_element_set_01", ("属性元素素材套装",)),
    (3037, "notice_s_collab_gacha_campaign", ("凉宫春日的忧郁", "联动纪念")),
    (10004, "advent_scollab_event", ("凉宫春日的跳跃",)),
)


# //// 绘制蓝绿色像素菱格背景 [@x380kkm 2026-09-21] ////
def background():
    image = Image.new("RGB", (WIDTH, HEIGHT))
    draw = ImageDraw.Draw(image)
    for y in range(HEIGHT):
        ratio = y / (HEIGHT - 1)
        color = tuple(round(start + (end - start) * ratio) for start, end in zip((112, 233, 218), (41, 109, 219)))
        draw.line((0, y, WIDTH, y), fill=color)
    overlay = Image.new("RGBA", image.size)
    pattern = ImageDraw.Draw(overlay)
    for row, y in enumerate(range(-96, HEIGHT + 192, 192)):
        for x in range(-192 + (row % 2) * 96, WIDTH + 192, 192):
            pattern.polygon(((x, y), (x + 96, y + 96), (x, y + 192), (x - 96, y + 96)), fill=(255, 255, 255, 19))
    return Image.alpha_composite(image.convert("RGBA"), overlay)


# //// 绘制阶梯轮廓的星形标记 [@x380kkm 2026-09-21] ////
def pixel_star(draw, x, y, size, color):
    points = ((0, -3), (1, -3), (1, -1), (3, -1), (3, 1), (1, 1), (1, 3),
              (-1, 3), (-1, 1), (-3, 1), (-3, -1), (-1, -1), (-1, -3))
    draw.polygon([(x + px * size, y + py * size) for px, py in points], fill=color)


# //// 在指定宽度内居中绘制中文 [@x380kkm 2026-09-21] ////
def centered_text(draw, text, top, size, font_path, color, width=1120):
    font = ImageFont.truetype(str(font_path), size)
    while draw.textlength(text, font=font) > width and size > 24:
        size -= 2
        font = ImageFont.truetype(str(font_path), size)
    draw.text((WIDTH // 2, top), text, font=font, fill=color, anchor="mt")


# //// 绘制带活动标题的公告卡面 [@x380kkm 2026-09-21] ////
def render_frame(titles, font_path):
    image = background()
    draw = ImageDraw.Draw(image)
    for x, y, size in ((94, 458, 12), (1280, 186, 14), (1242, 620, 10), (158, 1598, 14), (1224, 1674, 11)):
        pixel_star(draw, x, y, size, "#fff7bb")
    draw.polygon(((120, 166), (1320, 166), (1288, 418), (88, 418)), fill="#278ca8")
    draw.polygon(((120, 150), (1320, 150), (1288, 402), (88, 402)), fill="#ffffff")
    title_top = 219 if len(titles) == 1 else 185
    for index, title in enumerate(titles):
        centered_text(draw, title, title_top + index * 95, 78, font_path, "#245f91")
    draw.rounded_rectangle((107, 519, 1351, 1499), radius=38, fill="#276db4")
    draw.rounded_rectangle((88, 500, 1332, 1480), radius=38, fill="#f6ffff", outline="#d0faf3", width=7)
    pixel_star(draw, 720, 1633, 10, "#ffffff")
    return image


# //// 生成素材缺失提示卡 [@x380kkm 2026-09-21] ////
def render_notice(titles, font_path):
    image = render_frame(titles, font_path)
    draw = ImageDraw.Draw(image)
    draw.rectangle((554, 666, 886, 918), fill="#c6eae9")
    draw.rectangle((532, 644, 864, 896), fill="#ffffff", outline="#3687b3", width=14)
    draw.polygon(((560, 848), (642, 746), (702, 811), (752, 770), (836, 858)), fill="#66c6c8")
    draw.rectangle((768, 686, 810, 728), fill="#ffd761")
    draw.rectangle((670, 858, 770, 958), fill="#ffd761", outline="#ffffff", width=8)
    centered_text(draw, "!", 865, 67, font_path, "#715916")
    centered_text(draw, "素材缺失", 1030, 134, font_path, "#23678f")
    return image.convert("RGB")


# //// 将完整官方宣传图排入公告卡面 [@x380kkm 2026-09-21] ////
def render_official_card(titles, font_path, source):
    image = render_frame(titles, font_path)
    with Image.open(source["image"]) as original:
        if original.size != (1200, 630):
            raise ValueError(f"官方宣传图尺寸与卡面布局不符: {original.size}")
        image.paste(original.convert("RGB"), (120, 580))
    return image.convert("RGB")


# //// 生成公告替换图片与资源来源清单 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-directory", type=Path, required=True)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--official-sources", type=Path, required=True)
    args = parser.parse_args()
    sources = json.loads(args.official_sources.read_text(encoding="utf-8-sig"))
    official = {item["logical_path"]: item for item in sources["items"]}
    args.output_directory.mkdir(parents=True, exist_ok=True)
    assets = []
    for owner, name, titles in ANNOUNCEMENTS:
        output = args.output_directory / f"{name}.png"
        logical = f"dynamic/feature_announcement/{name}.png"
        source = official.get(logical)
        record = {"logical_path": logical, "preview": str(output.resolve()), "dimensions": [WIDTH, HEIGHT],
                  "owners": [f"feature_announcement:{owner}"], "provenance": "derived-substitute",
                  "replacement_kind": "missing-material-notice", "display_title": " / ".join(titles)}
        if source:
            render_official_card(titles, args.font, source).save(output)
            record.update(replacement_kind="official-social-promotion-card", language=source["language"],
                          source_url=source["source_url"], original_image=source["image"],
                          image_url=source["image_url"], match_type=source["match_type"],
                          exact_missing_asset_match=source["exact_missing_asset_match"], evidence=source["evidence"])
        else:
            render_notice(titles, args.font).save(output)
        assets.append(record)
    prepared = {"source": {"official_social_media": str(args.official_sources.resolve()),
                           "layout": "native_announcement_card"}, "assets": assets, "remaining_missing": []}
    output = args.output_directory / "prepared.json"
    output.write_text(json.dumps(prepared, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"prepared": str(output), "images": len(assets)}))


if __name__ == "__main__":
    main()
# //// /生成公告替换图片与资源来源清单 ////
