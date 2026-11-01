#!/usr/bin/env python3
"""对同一入口扫多种握手/订阅形态，看哪一版能收到帧。

为什么需要它：服务端 101 成功、token 也校验（4001），但 hello/subscribe/ping
全都没有回应。这种现象有两种成因 —— 要么上游没实现这套文本协议，
要么它用的是另一套字段名。挨个用浏览器试太慢，这里一次把候选形态跑完，
每个变体独立连接、独立计时，收到的帧带时间戳，归属清楚。
"""

import json
import os
import socket
import ssl
import struct
import sys
import time

URL = "wss://<server>:8443/ws"
TOKEN = "net-fc5f6d29c906b68eb5cc103a5473ff7d2ba7134130eab502"
WAIT = 9.0

TOPICS = [
    "task.started", "task.running", "task.completed", "task.failed",
    "task.warning", "alert.critical", "alert.warn", "deploy.done", "flimx.*",
]


def connect(url, token, header_auth=True):
    from urllib.parse import urlparse
    u = urlparse(url)
    host, port = u.hostname, u.port or 443
    path = u.path or "/"
    if u.query:
        path += "?" + u.query
    raw = socket.create_connection((host, port), timeout=10)
    sock = ssl._create_unverified_context().wrap_socket(raw, server_hostname=host)
    import base64
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
        "User-Agent: EchoCast-sweep/1\r\n"
    )
    if token and header_auth:
        req += f"Authorization: Bearer {token}\r\n"
    req += "\r\n"
    sock.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        c = sock.recv(4096)
        if not c:
            break
        buf += c
    head, _, rest = buf.partition(b"\r\n\r\n")
    return sock, head.decode("latin-1").split("\r\n")[0], rest


def send(sock, text):
    data = text.encode()
    n = len(data)
    head = bytearray([0x81])
    if n < 126:
        head.append(0x80 | n)
    elif n < 65536:
        head.append(0x80 | 126)
        head += struct.pack(">H", n)
    else:
        head.append(0x80 | 127)
        head += struct.pack(">Q", n)
    mask = os.urandom(4)
    head += mask
    head += bytes(c ^ mask[i % 4] for i, c in enumerate(data))
    sock.sendall(bytes(head))


def pump(sock, rest, seconds):
    """收 seconds 秒，返回 [(t, op, text)]。"""
    buf = bytearray(rest)
    got = []
    t0 = time.time()
    sock.settimeout(0.5)
    while time.time() - t0 < seconds:
        try:
            chunk = sock.recv(65536)
            if not chunk:
                break
            buf.extend(chunk)
        except socket.timeout:
            pass
        except Exception:
            break
        while True:
            if len(buf) < 2:
                break
            b0, b1 = buf[0], buf[1]
            op, ln, idx = b0 & 0x0F, b1 & 0x7F, 2
            if ln == 126:
                if len(buf) < 4:
                    break
                ln, idx = struct.unpack(">H", bytes(buf[2:4]))[0], 4
            elif ln == 127:
                if len(buf) < 10:
                    break
                ln, idx = struct.unpack(">Q", bytes(buf[2:10]))[0], 10
            if len(buf) < idx + ln:
                break
            payload = bytes(buf[idx:idx + ln])
            del buf[:idx + ln]
            got.append((round(time.time() - t0, 1), op, payload[:400]))
    return got


CASES = [
    ("A 仅 header token，不发帧", "", [], True),
    ("B token 放 query，不发帧", f"?token={TOKEN}", [], False),
    ("C header token + {op:auth}", "", [json.dumps({"op": "auth", "token": TOKEN})], True),
    ("D header token + {type:subscribe}", "", [json.dumps({"type": "subscribe", "topics": TOPICS})], True),
    ("E header token + {op:sub,channels}", "", [json.dumps({"op": "sub", "channels": TOPICS})], True),
    ("F header token + subscribe 后 ping", "", [
        json.dumps({"op": "subscribe", "topics": TOPICS}),
        json.dumps({"op": "ping"}),
    ], True),
    ("G header token + {op:hello,terminal:...}", "", [json.dumps({
        "op": "hello",
        "terminal": {"id": "term_probe", "alias": "probe", "labels": ["probe"]},
        "topics": TOPICS,
    })], True),
]


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else ""
    for name, qs, frames, hdr in CASES:
        if only and only not in name:
            continue
        print(f"\n===== {name}")
        try:
            sock, status, rest = connect(URL + qs, TOKEN, header_auth=hdr)
        except Exception as e:  # noqa: BLE001
            print(f"  连接失败: {type(e).__name__}: {e}")
            continue
        print(f"  {status}")
        if "101" not in status:
            continue
        for f in frames:
            send(sock, f)
            print(f"  → {f[:160]}")
            time.sleep(0.6)
        got = pump(sock, rest, WAIT)
        if not got:
            print(f"  {WAIT:.0f}s 内 0 帧")
        for t, op, payload in got:
            print(f"  [{t:>4}s] op=0x{op:x} {payload.decode('utf-8', 'replace')[:300]}")
        try:
            sock.close()
        except Exception:
            pass


if __name__ == "__main__":
    main()
