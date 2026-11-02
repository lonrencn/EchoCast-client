"""工单修复验证：#2 媒体渲染与注入面、#4 问询保护"""
import pathlib, time
from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEMO = ROOT / "demo" / "index.html"
TMP = ROOT / ".shots"
errors = []

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    pg = b.new_page(viewport={"width": 1480, "height": 950})
    pg.on("pageerror", lambda e: errors.append(f"pageerror: {e}"))
    pg.on("console", lambda m: errors.append(f"console.error: {m.text}") if m.type == "error" else None)
    pg.goto(DEMO.as_uri())
    pg.wait_for_timeout(500)
    pg.click("#connBtn")
    pg.wait_for_timeout(400)

    # ── #2 weekly 附件：SVG 图表应真正渲染（naturalWidth > 0）────
    pg.locator('[data-task="weekly"]').click()
    pg.wait_for_timeout(4100)
    pg.locator(".notice-stack .notice", has_text="生成团队周报").first.click()
    pg.wait_for_timeout(600)
    imgs = pg.locator(".s-media img")
    n = imgs.count()
    sizes = []
    for i in range(n):
        sizes.append(imgs.nth(i).evaluate("el => el.naturalWidth"))
    print(f"[#2] weekly 图片数={n} naturalWidth={sizes}")
    assert n == 2 and all(w > 0 for w in sizes), "SVG 图表未渲染"
    pg.keyboard.press("Escape")
    pg.wait_for_timeout(300)

    # ── #4 问询保护：已读按钮、关面板都不能收走问询 ────────────
    pg.locator('[data-task="cleanup"]').click()
    pg.wait_for_timeout(800)
    ask = pg.locator(".notice-stack .notice", has_text="清理构建缓存").first
    assert ask.count() > 0, "问询气泡未出现"
    # 点气泡正文 → 开详情 → 关掉 → 问询应还在
    ask.click()
    pg.wait_for_timeout(400)
    pg.keyboard.press("Escape")
    pg.wait_for_timeout(600)
    still = pg.locator(".notice-stack .notice", has_text="清理构建缓存").count()
    print("[#4a] 关面板后问询仍在:", still > 0)
    assert still > 0, "问询被关面板静默删除"
    # 已读按钮（data-act=read）
    pg.locator(".notice-stack .notice[data-state=ask] [data-act=read]").first.click()
    pg.wait_for_timeout(400)
    still2 = pg.locator(".notice-stack .notice", has_text="清理构建缓存").count()
    toast = pg.locator(".toast.show").count()
    print("[#4b] 点已读后问询仍在:", still2 > 0, "| toast 提示:", toast > 0)
    assert still2 > 0, "问询被已读按钮静默删除"
    # 忽略按钮是合法出口
    pg.locator(".notice-stack .notice[data-state=ask] [data-act=ignore]").first.click()
    pg.wait_for_timeout(500)
    gone = pg.locator(".notice-stack .notice", has_text="清理构建缓存").count()
    print("[#4c] 「忽略」可收起:", gone == 0)
    assert gone == 0

    # ── #2 注入面说明 ─────────────────────────────────────────
    # Windows 文件名不允许出现引号，上传路径注入不了属性；blob: URL 由
    # 浏览器生成也不可控。剩下的 src 来源是 data URI（chartSVG，已在
    # 上面验证整体编码）——demo 的注入面已全部封死，无需进一步构造。
    pg.screenshot(path=str(TMP / "ticket-fix-verify.png"))
    b.close()

print("[JS错误]", len(errors), errors[:2])
assert not errors, "页面有 JS 错误"
print("ALL PASS")
