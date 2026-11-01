"""抓取指定窗口自身的画面。

为什么不用 ImageGrab：它抓的是屏幕像素，窗口一旦被别的窗口压住，
或者截图坐标和 DPI 缩放差一点，就会截到别人的内容。
PrintWindow 让窗口自己往一块内存位图里绘制，天然免疫遮挡。
"""

import ctypes
import subprocess
import sys
from ctypes import wintypes

from PIL import Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

PW_RENDERFULLCONTENT = 0x00000002


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


def find_window(title="EchoCast"):
    """按窗口标题 + 窗口类名查找 Tauri 窗口。

    早先版本靠 tasklist 取 pid，但中文系统的 tasklist 输出是 GBK，
    被按 UTF-8 解码时会直接抛异常。纯 Win32 枚举没有这个依赖。
    """
    hits = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, _):
        n = user32.GetWindowTextLengthW(hwnd)
        if n == 0:
            return True
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        if buf.value != title:
            return True
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        if "Tauri" in cls.value:
            hits.append(hwnd)
        return True

    user32.EnumWindows(cb, 0)
    return hits[0] if hits else None


def capture(hwnd, path):
    r = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    w, h = r.right - r.left, r.bottom - r.top

    hdc = user32.GetWindowDC(hwnd)
    mdc = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
    old = gdi32.SelectObject(mdc, bmp)

    ok = user32.PrintWindow(hwnd, mdc, PW_RENDERFULLCONTENT)
    if not ok:
        # 部分 WebView 版本不支持 FULLCONTENT，退一级
        user32.PrintWindow(hwnd, mdc, 0)

    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = w
    bi.bmiHeader.biHeight = -h  # 负值 = top-down，省一次翻转
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    bi.bmiHeader.biCompression = 0

    buf = ctypes.create_string_buffer(w * h * 4)
    gdi32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)

    img = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1)
    img.convert("RGB").save(path)

    gdi32.SelectObject(mdc, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mdc)
    user32.ReleaseDC(hwnd, hdc)
    return w, h


if __name__ == "__main__":
    hwnd = find_window()
    if not hwnd:
        print("没找到 EchoCast 窗口")
        sys.exit(1)
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    print(f"hwnd={hwnd} rect=({rect.left},{rect.top},{rect.right},{rect.bottom})")
    size = capture(hwnd, sys.argv[1] if len(sys.argv) > 1 else "win.png")
    print(f"captured {size[0]}x{size[1]}")
