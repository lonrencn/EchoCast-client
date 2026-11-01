"""demo 上传文件功能 E2E：连接 → 选文件 → 推送 → 气泡附件 → sheet 四类渲染 + 无 JS 错误"""
import sys, time, pathlib
from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEMO = ROOT / "demo" / "index.html"
TMP = ROOT / ".shots"

# 准备测试文件：一张图、一段音频(空壳带正确头)、一个普通文件
img = TMP / "up-test.png"
import struct, zlib
def png(w, h, rgb):
    sig = b"\x89PNG\r\n\x1a\n"
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    return sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
img.write_bytes(png(40, 40, (47, 217, 164)))
wav = TMP / "up-test.wav"
wav.write_bytes(b"RIFF$\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x44\xac\x00\x00\x88\x58\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00")
doc = TMP / "up-test.csv"
doc.write_text("a,b\n1,2\n", encoding="utf8")

errors = []
with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    pg = b.new_page(viewport={"width": 1480, "height": 950}, device_scale_factor=2)
    pg.on("pageerror", lambda e: errors.append(f"pageerror: {e}"))
    pg.on("console", lambda m: errors.append(f"console.{m.type}: {m.text}") if m.type == "error" else None)
    pg.goto(DEMO.as_uri())
    pg.wait_for_timeout(600)

    # 连接
    pg.click("#connBtn") if pg.locator("#connBtn").count() else pg.get_by_role("button", name="连接").click()
    pg.wait_for_timeout(500)

    # 选 3 个文件
    pg.set_input_files("#fileInput", [img, wav, doc])
    pg.wait_for_timeout(300)
    chips = pg.locator(".f-chip").count()
    print("[1] chips:", chips)
    assert chips == 3, "应显示 3 个文件 chip"

    # 不填任务名直接推（自动命名 交付 3 个文件）
    pg.click("#customSend")
    pg.wait_for_timeout(4100)  # 等任务走完 3.3s

    bubble = pg.locator(".notice-stack .notice", has_text="交付 3 个文件").first
    print("[2] 气泡出现:", bubble.count() > 0)
    print("    附件行:", bubble.locator("text=3 个附件").count() > 0)

    # 点开详情
    bubble.click()
    pg.wait_for_timeout(500)
    print("[3] sheet 开:", pg.locator(".sheet-backdrop.open").count() > 0)
    print("    图片:", pg.locator(".s-media img").count())
    print("    音频卡:", pg.locator(".s-media .f-card audio").count())
    print("    文件卡+下载:", pg.locator(".s-media .f-card a.dl").count())
    shot = TMP / "demo-upload-sheet.png"
    pg.screenshot(path=str(shot))
    print("[4] 截图:", shot.name)

    # 移除 chip 再加一个
    pg.keyboard.press("Escape")
    pg.wait_for_timeout(300)
    pg.set_input_files("#fileInput", [doc])
    pg.locator(".f-chip button").first.click()
    pg.wait_for_timeout(200)
    print("[5] 移除后 chips:", pg.locator(".f-chip").count())

    # weekly 模板回归（image kind 渲染）
    pg.locator('[data-task="weekly"]').click()
    pg.wait_for_timeout(4100)
    pg.locator(".notice-stack .notice", has_text="生成团队周报").first.click()
    pg.wait_for_timeout(400)
    print("[6] weekly sheet 图片:", pg.locator(".s-media img").count())
    pg.screenshot(path=str(TMP / "demo-upload-weekly.png"))
    b.close()

print("[JS错误]", len(errors), errors[:3])
assert not errors, "页面有 JS 错误"
print("ALL PASS")
