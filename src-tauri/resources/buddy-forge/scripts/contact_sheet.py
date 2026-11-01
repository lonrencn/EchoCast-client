#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
对版图：把素材包里的每个动作贴到纸色底上，画基线与安全边，
用来肉眼检查透明边缘、洋红残留、脚底是否对齐。

用法: python contact_sheet.py --dir ../../src-tauri/resources/buddies/default
"""
from pathlib import Path
import argparse
import json
import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from PIL import Image, ImageDraw

PAPER = (251, 242, 228)
GRID = (222, 208, 190)
BASE_LINE = (178, 106, 69)
LABEL = (90, 74, 60)
SCALE = 3          # 放大倍数，方便看边缘
PAD = 18
CAPTION = 22


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--out", default="preview.png")
    a = ap.parse_args()

    d = Path(a.dir)
    mf = d / "manifest.json"
    if not mf.exists():
        print(f"[x] 没有 manifest.json: {d}", file=sys.stderr)
        return 1

    manifest = json.loads(mf.read_text(encoding="utf-8"))
    frame, baseline = manifest["frame"], manifest["baseline"]
    actions = manifest["actions"]
    cell = frame * SCALE
    cols = len(actions)

    sheet = Image.new("RGB", (cols * (cell + PAD) + PAD, cell + PAD * 2 + CAPTION), PAPER)
    dr = ImageDraw.Draw(sheet)

    for i, (state, meta) in enumerate(actions.items()):
        sprite = Image.open(d / meta["file"]).convert("RGBA")
        sprite = sprite.resize((cell, cell), Image.NEAREST)
        x = PAD + i * (cell + PAD)
        y = PAD
        sheet.paste(sprite, (x, y), sprite)

        # 基线：脚底应正好压在这条线上
        by = y + baseline * SCALE
        dr.line([(x, by), (x + cell, by)], fill=BASE_LINE, width=2)
        dr.rectangle([x, y, x + cell - 1, y + cell - 1], outline=GRID, width=1)
        dr.text((x + 2, y + cell + 6), state, fill=LABEL)

    sheet.save(a.out)
    print(f"[ok] {a.out}  ({sheet.width}x{sheet.height}, x{SCALE})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
