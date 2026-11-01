#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
buddy-forge — 把生图产物处理成 EchoCast 可用的形象素材包。

子命令:
  prompt    按模板生成 6 个动作的提示词，直接拿去生图
  process   单张图 -> 抠透明底 -> 裁切对齐 -> WebP
  pack      整个目录批量处理，产出 webp + manifest.json
  check     校验素材包完整性（尺寸 / alpha / 体积 / 必需动作）

依赖: pillow, numpy
"""
from __future__ import annotations

import argparse
import datetime as _dt
import json
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

import numpy as np
from PIL import Image

# 出图边长。按 4x 出，显示 40–96px 都不会糊（辉光模式下角色要放到 88px+）
FRAME = 256


def _find_builtin_dir() -> Path | None:
    """向上找 EchoCast 仓库里的 src/buddies/builtin。

    skill 装在用户级目录，可能在任何项目里被调用：
    在 EchoCast 仓库里跑 → 找到内置包目录，出包即生效（前端构建期 glob 自动发现）；
    在别处跑 → 返回 None，`pack` 不带 --out 时落到当前目录的 buddy-packs/<名字>，
    用户再自己拷进 %APPDATA%\\com.echocast.client\\buddies\\。
    """
    for parent in Path(__file__).resolve().parents:
        cand = parent / "src" / "buddies" / "builtin"
        if cand.is_dir():
            return cand
    return None


BUILTIN_DIR = _find_builtin_dir()
SAFE = round(FRAME * 0.042)   # 底部安全边，按比例随 FRAME 缩放（96 时=4，256 时=11）
BASELINE = FRAME - SAFE   # 脚底基线，所有帧共用
CONTENT_RATIO = 0.88      # 主体占画布比例，留呼吸空间
MAX_H = round(FRAME * CONTENT_RATIO)  # 主体最大高度
MAGENTA = np.array([255, 0, 255], dtype=np.float32)

# 动作白名单。required 缺任何一个都不算合格包。
STATES_REQUIRED = ["idle", "success", "fail"]
STATES_OPTIONAL = ["running", "idle_blink", "hover", "sleep"]
STATES = STATES_REQUIRED + STATES_OPTIONAL

# 每个动作的情绪说明，进提示词模板
ACTIONS = {
    "idle": "正面站立，微微抬头，睁眼平视，嘴角轻抿",
    "running": "正面站立，双眼睁大呈圆环状，神情专注，双臂前伸似在操作",
    "idle_blink": "正面站立，双眼闭合，神情平静",
    "hover": "抬头看向上方，笑眼弯起，身体略微前倾",
    "success": "笑眼弯成弧形，张嘴笑，一只手向上举起",
    "fail": "低头，肩膀下垂，眼睛下垂，嘴角平直",
    "sleep": "蜷缩身体，闭眼，头微微低垂",
}

PROMPT_TMPL = """【角色】{species}小生物，主色{color}，钝角造型，简笔扁平插画
【动作】{action}
【构图】正面全身，主体占画面 75%，水平居中，脚底贴齐画面下沿
【风格】扁平矢量，2px 深色描边，纯色填充；无渐变、无投影、无高光、无地面阴影
【背景】纯洋红 #FF00FF 平涂，无纹理
【禁止】文字、水印、多余装饰；颜色不超过 3 种
【尺寸】1024x1024"""


# ---------- 抠图 ----------

def alpha_transparent_ratio(im: Image.Image) -> float:
    """返回接近全透明像素的占比，用来判断这张图是不是已经带 alpha。"""
    if im.mode not in ("RGBA", "LA"):
        return 0.0
    a = im.convert("RGBA").getchannel("A")
    hist = a.histogram()
    total = sum(hist)
    return (sum(hist[:250]) / total) if total else 0.0


def sample_key(rgb: np.ndarray) -> np.ndarray:
    """从四角采样背景色并取中位数。

    生图模型不会老实给你 #FF00FF，实测给的是 (190,28,136) 这种偏深的品红。
    硬编码键色必然失手，所以键色只能现场采样。
    """
    h, w = rgb.shape[:2]
    s = max(8, min(h, w) // 40)
    corners = np.concatenate([
        rgb[:s, :s].reshape(-1, 3),
        rgb[:s, -s:].reshape(-1, 3),
        rgb[-s:, :s].reshape(-1, 3),
        rgb[-s:, -s:].reshape(-1, 3),
    ])
    return np.median(corners, axis=0)


def erase_watermark(rgb: np.ndarray, key: np.ndarray,
                    box=(0.74, 0.88, 1.0, 1.0)) -> np.ndarray:
    """把右下角水印区刷成键色。

    平台水印是白字，抠图后会留下一个孤立区块，把 alpha bbox 撑到画布右下角，
    角色因此被缩到很小。必须在算 bbox 之前抹掉。
    """
    h, w = rgb.shape[:2]
    x0, y0, x1, y1 = box
    rgb[int(h * y0):int(h * y1), int(w * x0):int(w * x1)] = key
    return rgb


def keyout(im: Image.Image, inner: float = 42.0, outer: float = 132.0) -> Image.Image:
    """色键抠图：按到采样键色的距离给软 alpha，并对边缘做 despill 去溢色。"""
    arr = np.asarray(im.convert("RGBA"), dtype=np.float32)
    rgb, a0 = arr[..., :3].copy(), arr[..., 3] / 255.0

    key = sample_key(rgb)
    rgb = erase_watermark(rgb, key)

    dist = np.sqrt(((rgb - key) ** 2).sum(axis=-1))
    t = np.clip((dist - inner) / (outer - inner), 0.0, 1.0)
    alpha = (t * t * (3.0 - 2.0 * t)) * a0          # smoothstep，软边不锯齿

    # despill：边缘像素上高于绿通道的红/蓝，按透明程度拉回去，消除品红残留
    g = rgb[..., 1]
    k = 1.0 - alpha
    r = rgb[..., 0] - np.maximum(0.0, rgb[..., 0] - g) * k * 0.85
    b = rgb[..., 2] - np.maximum(0.0, rgb[..., 2] - g) * k * 0.85

    out = np.stack([r, g, b, alpha * 255.0], axis=-1)
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


# ---------- 裁切对齐 ----------

def align(im: Image.Image, frame: int = FRAME, baseline: int = BASELINE) -> Image.Image:
    """按 alpha bbox 裁切，等比缩放后底边对齐 baseline、水平居中。"""
    mask = im.getchannel("A").point(lambda v: 255 if v > 8 else 0)
    bbox = mask.getbbox()
    if not bbox:
        raise ValueError("抠图后 alpha 全空，可能是背景不是纯洋红")

    sprite = im.crop(bbox)
    cw, ch = sprite.size
    scale = min(MAX_H / ch, (frame - 2) / cw, 1.0)   # 不放大，避免糊
    nw, nh = max(1, round(cw * scale)), max(1, round(ch * scale))
    sprite = sprite.resize((nw, nh), Image.LANCZOS)

    canvas = Image.new("RGBA", (frame, frame), (0, 0, 0, 0))
    canvas.alpha_composite(sprite, ((frame - nw) // 2, baseline - nh))
    return canvas


# ---------- 主流程 ----------

def process_one(src: Path, dst: Path, quality: int = 85) -> dict:
    im = Image.open(src)
    keyed = alpha_transparent_ratio(im) > 0.02
    rgba = im.convert("RGBA") if keyed else keyout(im)
    out = align(rgba)

    dst.parent.mkdir(parents=True, exist_ok=True)
    out.save(dst, "WEBP", quality=quality, method=6)

    return {
        "src": src.name,
        "out": dst.name,
        "keyed": not keyed,
        "size_kb": round(dst.stat().st_size / 1024, 1),
    }


def build_manifest(pack: str, actions: dict, display: int, primary: bool = False) -> dict:
    return {
        "pack": pack,
        "primary": primary,
        "version": 1,
        "frame": FRAME,
        "display": display,
        "baseline": BASELINE,
        "generated_at": _dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "actions": actions,
    }


ACTION_META = {
    "idle":       {"frames": 1, "duration": 2400, "loop": True},
    "idle_blink": {"frames": 1, "duration": 3200, "loop": True},
    "hover":      {"frames": 1, "duration": 400,  "loop": False, "hold": "last"},
    "running":    {"frames": 1, "duration": 1600, "loop": True},
    "success":    {"frames": 1, "duration": 560,  "loop": False, "hold": "last"},
    "fail":       {"frames": 1, "duration": 500,  "loop": False, "hold": "last"},
    "sleep":      {"frames": 1, "duration": 3000, "loop": True},
}


def cmd_prompt(a) -> int:
    for state in STATES:
        print(f"--- {state} ---")
        print(PROMPT_TMPL.format(species=a.species, color=a.color, action=ACTIONS[state]))
        print()
    return 0


def cmd_process(a) -> int:
    src, dst = Path(a.src), Path(a.out)
    if not src.exists():
        print(f"[x] 找不到输入: {src}", file=sys.stderr)
        return 1
    try:
        info = process_one(src, dst, a.quality)
    except ValueError as e:
        print(f"[x] {src.name}: {e}", file=sys.stderr)
        return 1
    flag = "色键抠图" if info["keyed"] else "已有 alpha"
    print(f"[ok] {info['src']} -> {info['out']}  ({flag}, {info['size_kb']} KB)")
    return 0


def cmd_pack(a) -> int:
    src_dir = Path(a.src)
    if a.out:
        out_dir = Path(a.out)
    elif BUILTIN_DIR is not None:
        out_dir = BUILTIN_DIR / (a.name or src_dir.name)
    else:
        out_dir = Path.cwd() / "buddy-packs" / (a.name or src_dir.name)
        print(f"[i] 不在 EchoCast 仓库里，输出到 {out_dir}")
        print(f"[i] 装机：把整个文件夹拷到 %APPDATA%\\com.echocast.client\\buddies\\ 下")
    if not src_dir.is_dir():
        print(f"[x] 找不到素材目录: {src_dir}", file=sys.stderr)
        return 1

    found, missing = {}, []
    for state in STATES:
        candidate = None
        for ext in (".png", ".webp", ".jpg", ".jpeg"):
            p = src_dir / f"{state}{ext}"
            if p.exists():
                candidate = p
                break
        if candidate is None:
            missing.append(state)
            continue
        try:
            info = process_one(candidate, out_dir / f"{state}.webp", a.quality)
        except ValueError as e:
            print(f"[x] {state}: {e}", file=sys.stderr)
            return 1
        found[state] = info
        print(f"[ok] {state:<10} {info['size_kb']:>5} KB  {'keyed' if info['keyed'] else 'alpha'}")

    lacking = [s for s in STATES_REQUIRED if s not in found]
    if lacking:
        print(f"[x] 缺少必需动作: {', '.join(lacking)}", file=sys.stderr)
        return 1

    actions = {s: {"file": f"{s}.webp", **ACTION_META[s]} for s in found}
    manifest = build_manifest(a.name or out_dir.name, actions, a.display, a.primary)
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    total = sum(v["size_kb"] for v in found.values())
    print(f"\n[ok] 素材包 {manifest['pack']}: {len(found)} 个动作, 合计 {total:.1f} KB")
    if missing:
        print(f"[!] 可选动作未提供: {', '.join(missing)}（会回退到 idle）")
    print(f"[ok] 清单: {out_dir / 'manifest.json'}")
    return 0


def cmd_check(a) -> int:
    d = Path(a.dir)
    mf = d / "manifest.json"
    if not mf.exists():
        print(f"[x] 没有 manifest.json: {d}", file=sys.stderr)
        return 1

    manifest = json.loads(mf.read_text(encoding="utf-8"))
    problems = []
    for state, meta in manifest.get("actions", {}).items():
        p = d / meta["file"]
        if not p.exists():
            problems.append(f"{state}: 文件缺失 {meta['file']}")
            continue
        im = Image.open(p)
        if im.size != (manifest["frame"], manifest["frame"]):
            problems.append(f"{state}: 尺寸 {im.size} != {manifest['frame']}px")
        if im.mode != "RGBA" or alpha_transparent_ratio(im) <= 0.01:
            problems.append(f"{state}: 没有有效 alpha 通道")
        kb = p.stat().st_size / 1024
        if kb > 60:
            problems.append(f"{state}: {kb:.0f} KB 偏大，建议压到 60 KB 以内")

    for s in STATES_REQUIRED:
        if s not in manifest.get("actions", {}):
            problems.append(f"缺少必需动作 {s}")

    if problems:
        for p in problems:
            print(f"[x] {p}")
        return 1
    print(f"[ok] {manifest['pack']} 校验通过（{len(manifest['actions'])} 个动作）")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(prog="forge", description="EchoCast 形象素材管线")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p1 = sub.add_parser("prompt", help="生成 6 个动作的提示词")
    p1.add_argument("--species", default="方糖形", help="角色形态，如 方糖形/豆形/毛球")
    p1.add_argument("--color", default="苔绿", help="主色，如 苔绿/陶土/靛蓝")
    p1.set_defaults(func=cmd_prompt)

    p2 = sub.add_parser("process", help="处理单张图")
    p2.add_argument("src")
    p2.add_argument("--out", required=True)
    p2.add_argument("--quality", type=int, default=85)
    p2.set_defaults(func=cmd_process)

    p3 = sub.add_parser("pack", help="批量处理成素材包")
    p3.add_argument("--src", required=True, help="原始图目录，文件名需为 idle.png / success.png ...")
    p3.add_argument("--out", default="", help="输出目录；留空则输出到 src/buddies/builtin/<name>")
    p3.add_argument("--name", default="", help="包名，默认用输出目录名")
    p3.add_argument("--display", type=int, default=40, help="显示尺寸 px")
    p3.add_argument("--primary", action="store_true", help="标记为主推形象包，默认展示它")
    p3.add_argument("--quality", type=int, default=85)
    p3.set_defaults(func=cmd_pack)

    p4 = sub.add_parser("check", help="校验素材包")
    p4.add_argument("--dir", required=True)
    p4.set_defaults(func=cmd_check)

    a = ap.parse_args()
    return a.func(a)


if __name__ == "__main__":
    raise SystemExit(main())
