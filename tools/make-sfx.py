#!/usr/bin/env python3
"""生成 EchoCast 的四种提示音。

为什么自己合成而不是找音效包：这几个音只有 200–700ms，要的是「一耳朵知道
是什么事，但绝不刺耳」。现成音效库多半是手机通知那种四平八稳的叮咚，
放进这个暗色调界面里会显得很廉价；自己写反而能把音量包络、泛音比例、
尾音长度按场景调准。

四个音的语义分工：
    start    我刚接手 —— 两个上行轻点，最轻，因为这一步会反复出现
    success  成了     —— 三音上行，带点暖，是唯一允许「好听」的一个
    warn     得看一眼 —— 同音两下，间距明显，像敲桌子提醒
    fail     没成     —— 两音下行，压得低一点，不放亮色泛音
    ask      等你答复 —— 同一个高音连敲三下（dididi），刻意不做旋律

ask 为什么不跟前面四个共用「旋律」写法：前面四个都在说「事情怎么样」，
ask 说的是「轮到你了」。它必须从四个音里一眼（一耳）分辨出来，所以
音高固定不动、只有三下短促重复 —— 旋律会被听成又一个结果，重复才是催促。
它还会在没答复时反复播（间隔十几秒），单次必须足够短，否则第二次就成了噪音。

产物写到 src/assets/sfx/，前端直接 import 使用。
改音色就改这个文件重跑，不要在别处改 wav。

用法：
    python tools/make-sfx.py
"""

import math
import os
import struct
import wave

SR = 32000  # 32k 足够：最高泛音 4.2 倍频也才 3.5k，留足余量；体积是 44.1k 的 3/4
OUT_DIR = os.path.join(os.path.dirname(__file__), "..", "src", "assets", "sfx")

# 目标峰值。留足余量，用户把系统音量开大也不会削顶。
PEAK = 0.30


def note(freq: float, dur: float, decay: float = 7.0, amp: float = 1.0,
         partials=((1.0, 1.0), (2.0, 0.30), (3.0, 0.10), (4.2, 0.05)),
         bright: float = 1.0):
    """一个带泛音衰减的音符。

    每个泛音单独指数衰减，且频率越高衰减越快 —— 这一条是「木琴/铃」和
    「正弦波」的区别。全用同一个衰减率会听起来像电子表。
    """
    n = int(SR * dur)
    buf = [0.0] * n
    for k, (ratio, pa) in enumerate(partials):
        f = freq * ratio
        if f >= SR / 2:  # 超过奈奎斯特就丢掉，否则会叠出刺耳的镜像频率
            continue
        d = decay * (1.0 + 0.45 * k) * (1.0 / max(bright, 0.05))
        w = 2.0 * math.pi * f
        a = pa * amp
        for i in range(n):
            t = i / SR
            buf[i] += a * math.exp(-d * t) * math.sin(w * t)
    att = int(0.006 * SR)  # 6ms 起音，避免咔哒声
    for i in range(min(att, n)):
        buf[i] *= i / att
    return buf


def silence(dur: float) -> list:
    return [0.0] * int(SR * dur)


def mix(*layers):
    """layers: (offset_sec, buf) 列表，等长叠加。"""
    total = max(int(off * SR) + len(b) for off, b in layers)
    out = [0.0] * total
    for off, b in layers:
        s = int(off * SR)
        for i, v in enumerate(b):
            out[s + i] += v
    return out


def normalize(buf, peak=PEAK):
    m = max((abs(v) for v in buf), default=0.0)
    if m == 0:
        return buf
    k = peak / m
    return [v * k for v in buf]


def tail_fade(buf, ms=25):
    """结尾 25ms 淡出。指数衰减理论上到不了 0，硬切会留下一个咔哒。"""
    n = min(len(buf), int(SR * ms / 1000))
    for i in range(n):
        buf[len(buf) - n + i] *= 1.0 - i / n
    return buf


def write_wav(name: str, buf):
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, name)
    with wave.open(path, "w") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        frames = bytearray()
        for v in buf:
            s = int(max(-1.0, min(1.0, v)) * 32767)
            frames += struct.pack("<h", s)
        w.writeframes(bytes(frames))
    dur = len(buf) / SR
    rms = math.sqrt(sum(v * v for v in buf) / max(len(buf), 1))
    print(f"  {name:12s} {dur*1000:5.0f}ms  peak={max(abs(v) for v in buf):.2f}  rms={rms:.3f}")


# ---------------------------------------------------------------------------
# 四个音
# ---------------------------------------------------------------------------


def sfx_start():
    # A4 → E5，纯五度。上行 = 「我接住了」，但只给两个短点：
    # 这条会在每次任务开始时响，音一长就变成噪音。
    a = note(440.00, 0.26, decay=11.0, amp=0.9)
    b = note(659.25, 0.30, decay=9.5, amp=0.75)
    return mix((0.0, a), (0.075, b))


def sfx_success():
    # C5 - E5 - G5，大三和弦分解，最后一个音留长尾。
    # 唯一允许「好听」的一个：任务真的成了，值得一句收尾。
    a = note(523.25, 0.42, decay=7.5, amp=0.80)
    b = note(659.25, 0.46, decay=7.0, amp=0.78)
    c = note(783.99, 0.80, decay=4.6, amp=0.85)
    d = note(1046.50, 0.70, decay=5.4, amp=0.30)  # 上方八度借一点空气感
    return mix((0.0, a), (0.085, b), (0.170, c), (0.175, d))


def sfx_warn():
    # 同一个音敲两下，间距 110ms。音高不动 = 没有「完成/失败」的倾向，
    # 只是叫人来看一眼。加一点点五度让它不呆板。
    a = note(587.33, 0.22, decay=10.0, amp=0.85, bright=0.8)
    b = note(587.33, 0.40, decay=7.4, amp=0.72, bright=0.8)
    sub = note(293.66, 0.50, decay=6.0, amp=0.18, bright=1.6)
    return mix((0.0, a), (0.110, b), (0.0, sub))


def sfx_fail():
    # G4 → C4，下行四度。低、暗、不收尾：失败了不需要被安慰，只需要被知道。
    a = note(392.00, 0.34, decay=9.0, amp=0.80, bright=0.75)
    b = note(261.63, 0.60, decay=6.2, amp=0.85, bright=0.7)
    sub = note(130.81, 0.55, decay=6.0, amp=0.22, bright=1.4)
    return mix((0.0, a), (0.115, b), (0.100, sub))


def sfx_ask():
    # 同一个音连敲三下（dididi），音高固定不动。
    # 前面四个音都在说「事情怎么样」，只有这个在说「轮到你了」——
    # 所以它不能有旋律走向，只能是重复。选 D6：比 warn 高一个八度出头，
    # 在笔记本小喇叭上也能出来，但泛音收窄一点，避免变成刺耳的电子提示。
    a = note(1174.66, 0.10, decay=26.0, amp=0.85, bright=0.62)
    b = note(1174.66, 0.10, decay=26.0, amp=0.85, bright=0.62)
    c = note(1174.66, 0.16, decay=20.0, amp=0.90, bright=0.62)
    # 底下垫一点低八度，给它体积，不然三下会显得很「塑料」
    sub = note(587.33, 0.34, decay=9.0, amp=0.16, bright=1.5)
    return mix((0.0, a), (0.105, b), (0.210, c), (0.0, sub))


def main():
    print(f"输出到 {os.path.normpath(OUT_DIR)}  ({SR} Hz / mono / 16bit)")
    for name, fn in [
        ("start.wav", sfx_start),
        ("success.wav", sfx_success),
        ("warn.wav", sfx_warn),
        ("fail.wav", sfx_fail),
        ("ask.wav", sfx_ask),
    ]:
        buf = tail_fade(normalize(fn()))
        write_wav(name, buf)


if __name__ == "__main__":
    main()
