#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成 6 张洋红底占位图，用于验证 forge.py 的色键、裁切、对齐是否正确。
真实素材应由生图模型按 forge.py prompt 的输出生成，本文件只做管线自测。
"""
from pathlib import Path
import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from PIL import Image, ImageDraw

W = H = 1024
BG = (255, 0, 255)
BODY = (110, 127, 92)
INK = (251, 242, 228)
ACCENT = (178, 106, 69)
OUT = Path(__file__).parent / "raw"

# 眼睛样式 / 嘴部 / 手臂 / 身体压缩量（姿态必须靠形状差异，位移会被裁切抹平）
POSES = {
    "idle":       ("dot",   "smile", "none", 0),
    "idle_blink": ("line",  "smile", "none", 0),
    "hover":      ("happy", "grin",  "side", 0),
    "success":    ("happy", "open",  "up",   0),
    "fail":       ("sad",   "frown", "none", 70),
    "sleep":      ("line",  "tiny",  "none", 110),
}


def draw_buddy(state: str) -> Image.Image:
    eyes, mouth, arm, shrink = POSES[state]
    im = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(im)

    top, bottom = 300, 860 - shrink
    d.rounded_rectangle([250, top, 774, bottom], radius=150, fill=BODY)

    # 天线
    d.line([(512, top), (512, top - 90)], fill=BODY, width=26)
    d.ellipse([484, top - 122, 540, top - 66], fill=ACCENT)

    # 手臂：靠形状制造姿态差异
    if arm == "up":
        d.line([(750, top + 420), (820, top + 70)], fill=BODY, width=90)
        d.ellipse([820 - 45, top + 70 - 45, 820 + 45, top + 70 + 45], fill=BODY)
    elif arm == "side":
        d.line([(748, top + 420), (846, top + 330)], fill=BODY, width=88)
        d.ellipse([846 - 44, top + 330 - 44, 846 + 44, top + 330 + 44], fill=BODY)

    cy = top + 230
    for cx in (400, 624):
        if eyes == "dot":
            d.ellipse([cx - 30, cy - 30, cx + 30, cy + 30], fill=INK)
        elif eyes == "line":
            d.line([(cx - 34, cy), (cx + 34, cy)], fill=INK, width=26)
        elif eyes == "happy":
            d.arc([cx - 40, cy - 20, cx + 40, cy + 40], 200, 340, fill=INK, width=24)
        elif eyes == "sad":
            d.ellipse([cx - 26, cy - 12, cx + 26, cy + 40], fill=INK)
            d.line([(cx - 46, cy - 52), (cx + 46, cy - 30)], fill=INK, width=20)

    my = cy + 130
    if mouth == "smile":
        d.arc([432, my - 40, 592, my + 56], 20, 160, fill=INK, width=22)
    elif mouth == "grin":
        d.arc([412, my - 56, 612, my + 90], 15, 165, fill=INK, width=26)
    elif mouth == "open":
        d.ellipse([446, my - 20, 578, my + 76], fill=INK)
    elif mouth == "frown":
        d.arc([440, my + 20, 584, my + 110], 200, 340, fill=INK, width=22)
    else:
        d.arc([470, my, 554, my + 44], 25, 155, fill=INK, width=18)

    return im


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    for state in POSES:
        p = OUT / f"{state}.png"
        draw_buddy(state).save(p, "PNG")
        print(f"[fixture] {p.name}  {W}x{H}")
    print(f"\n共 {len(POSES)} 张 -> {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
