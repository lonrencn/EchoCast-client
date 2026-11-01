"""品牌截图合成：透明捕获 → 紧裁剪 → 深色渐变底 + 辉光"""
import pathlib
from PIL import Image, ImageFilter

SRC = pathlib.Path(__file__).resolve().parent.parent / ".shots"
DST = pathlib.Path(__file__).resolve().parent.parent / "docs" / "brand"
DST.mkdir(parents=True, exist_ok=True)

def compose(name, out, pad=48, glow=(47, 217, 164)):
    im = Image.open(SRC / name).convert("RGBA")
    # alpha 感知裁剪：内容 bbox + 边距
    bbox = im.getchannel("A").getbbox()
    if not bbox:
        raise SystemExit(f"{name} 是空图")
    l, t, r, b = bbox
    l, t = max(0, l - pad), max(0, t - pad)
    r, b = min(im.width, r + pad), min(im.height, b + pad)
    im = im.crop((l, t, r, b))

    # 画布：垂直深色渐变（应用主题色系）+ 底部中央 accent 辉光
    W, H = im.width + pad * 2, im.height + pad * 2
    bg = Image.new("RGB", (W, H))
    top, bottom = (13, 20, 36), (6, 10, 18)
    px = bg.load()
    for y in range(H):
        k = y / max(H - 1, 1)
        c = tuple(int(top[i] + (bottom[i] - top[i]) * k) for i in range(3))
        for x in range(W):
            px[x, y] = c
    glow_layer = Image.new("RGB", (W, H), (0, 0, 0))
    gp = glow_layer.load()
    cx, cy = W // 2, int(H * 0.82)
    for y in range(H):
        for x in range(W):
            d2 = (x - cx) ** 2 + (y - cy) ** 2
            g = max(0, 1 - d2 / (0.55 * W * 0.55 * W + 1))
            gp[x, y] = (int(glow[0] * g * 0.28), int(glow[1] * g * 0.28), int(glow[2] * g * 0.28))
    glow_layer = glow_layer.filter(ImageFilter.GaussianBlur(60))
    bg = Image.blend(bg, Image.blend(bg, glow_layer, 0.5), 0.9)
    from PIL import ImageChops
    bg = ImageChops.add(bg, glow_layer)

    bg.paste(im, (pad, pad), im)
    bg.save(DST / out, optimize=True)
    print(out, f"{bg.width}x{bg.height}", f"{(DST / out).stat().st_size/1024:.0f} KB")

compose("brand-bubble.png", "notification-bubble.png")
compose("brand-delivery.png", "delivery-list.png")
compose("brand-sheet.png", "detail-panel.png")
print("DONE ->", DST)
