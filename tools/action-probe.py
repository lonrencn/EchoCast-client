#!/usr/bin/env python3
"""上行动作探针：验证「用户点了按钮」这件事服务端到底收不收、认不认、回不回。

为什么单独一个脚本：客户端界面上点一下能看到什么，取决于三件不同的事 ——
帧发出去了没有（本地队列）、服务端收到了没有（对端）、服务端承认不承认
这个 notice_id（业务）。三件事错了在界面上长得一模一样，只有看原始帧能分开。

用法:
    # 用不存在的 id 试：服务端若回 nack，说明动作路由在线
    python tools/action-probe.py --fake

    # 等一条真实通知，用它的 id 回动作，看服务端会不会 ack
    python tools/action-probe.py --real --ttl 150

    # 指定地址/token（默认读 %APPDATA%/com.echocast.client/settings.json）
    python tools/action-probe.py --real --url wss://... --token tok_xxx
"""

import json
import os
import socket
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wsprobe import frames, handshake, send_text  # noqa: E402

TOPICS = [
    "task.started", "task.running", "task.completed", "task.failed",
    "task.warning", "alert.critical", "alert.warn", "deploy.done", "flimx.*",
]


def settings() -> dict:
    p = os.path.join(os.environ["APPDATA"], "com.echocast.client", "settings.json")
    with open(p, encoding="utf-8-sig") as f:
        return json.load(f)


def ts() -> str:
    return time.strftime("%H:%M:%S")


def classify(text: str) -> str:
    """给收到的帧贴个标签，方便一眼看出「有没有回执」。"""
    try:
        o = json.loads(text)
    except Exception:  # noqa: BLE001
        return "?"
    op = o.get("op") or "(no op)"
    if op.startswith("action"):
        return f"REPLY:{op}"
    return op


def main() -> int:
    args = sys.argv[1:]
    s = settings()
    url = s.get("server_url") or ""
    token = s.get("token") or ""
    if "--url" in args:
        url = args[args.index("--url") + 1]
    if "--token" in args:
        token = args[args.index("--token") + 1]

    fake = "--fake" in args
    real = "--real" in args
    ttl = 60.0
    if "--ttl" in args:
        ttl = float(args[args.index("--ttl") + 1])

    if not url:
        print("没有 server_url")
        return 2
    if not (fake or real):
        print("要么 --fake 要么 --real")
        return 2

    print(f"[{ts()}] {url}  token={'…' + token[-6:] if token else '(空)'}")
    try:
        sock, status, _h, rest = handshake(url, token)
    except Exception as e:  # noqa: BLE001
        print(f"[{ts()}] 握手失败 {type(e).__name__}: {e}")
        return 1
    print(f"[{ts()}] {status}")
    if "101" not in status:
        print("未升级成 WebSocket")
        return 1

    send_text(sock, json.dumps({
        "op": "hello", "protocol": "1", "alias": "action-probe",
        "labels": ["probe"], "device": {"device_id": "probe-0001", "hostname": "probe",
                                          "os": "windows", "app_version": "0.1.0"},
    }))
    print(f"[{ts()}] → hello")
    send_text(sock, json.dumps({"op": "subscribe", "topics": TOPICS}))
    print(f"[{ts()}] → subscribe")

    start = time.time()
    sock.settimeout(2.0)
    sent_for: str | None = None
    replies: list[str] = []
    notices = 0
    last_real: tuple[str, str] | None = None

    # 外层 while 必须存在：服务端安静时 `frames()` 会因 socket 超时结束生成器，
    # 而「安静」恰恰是假 id 模式要等的那几秒 —— 只写一层 for，动作永远发不出去。
    restbuf = rest
    try:
        while time.time() - start <= ttl:
            try:
                for op, payload in frames(sock, restbuf):
                    restbuf = b""
                    now = time.time() - start
                    if op != 0x1:
                        continue
                    text = payload.decode("utf-8", "replace")
                    kind = classify(text)

                    if kind.startswith("REPLY:"):
                        replies.append(f"{ts()}  {kind}  {text[:400]}")
                        print(f"[{ts()}] ★ 回执 {text[:400]}")
                        continue

                    if kind == "notice":
                        notices += 1
                        n = json.loads(text).get("notice") or {}
                        act = (n.get("actions") or [])
                        if n.get("id"):
                            last_real = (n["id"], (act[0]["id"] if act else "open"))
                        if notices <= 2:
                            print(f"[{ts()}] ← notice id={n.get('id')} topic={n.get('topic')} "
                                  f"actions={[a.get('id') for a in act]}")
                        # 真 id 模式：一拿到带 id 的通知就回一条
                        if real and sent_for is None and n.get("id"):
                            sent_for = n["id"]
                            frame = {"op": "action", "notice_id": n["id"], "action_id": last_real[1]}
                            send_text(sock, json.dumps(frame, ensure_ascii=False))
                            print(f"[{ts()}] → action notice_id={n['id']} action_id={last_real[1]}")
                        continue

                    print(f"[{ts()}] ← {kind} {text[:160]}")
            except socket.timeout:
                pass

            # 空闲也得分派：假 id 要按时发出，ttl 要按时到
            now = time.time() - start
            if fake and sent_for is None and now > 3:
                sent_for = "probe-does-not-exist-001"
                send_text(sock, json.dumps({
                    "op": "action", "notice_id": sent_for, "action_id": "open",
                }))
                print(f"[{ts()}] → action notice_id={sent_for} (假的)")
    except Exception as e:  # noqa: BLE001
        print(f"[{ts()}] 循环结束 {type(e).__name__}: {e}")

    elapsed = time.time() - start
    print("\n—— 结论 ——")
    print(f"收帧 {notices} 条，发出 action {1 if sent_for else 0} 条，收到回执 {len(replies)} 条，用时 {elapsed:.0f}s")
    if sent_for is None:
        print("没等到带 id 的通知，也没发动作。服务端可能没在推，或这台终端没被路由到。")
    elif not replies:
        print("动作发出去了，服务端一个字都没回 —— 客户端只能显示「已发出 · 待确认」。")
        print("这正是当前界面显示琥珀色而不是绿色的原因。")
    else:
        for r in replies:
            print("  " + r)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
