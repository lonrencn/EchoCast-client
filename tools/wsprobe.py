#!/usr/bin/env python3
"""裸 WebSocket 探针：验证服务端 /ws 到底通不通，顺便看它推什么帧。

为什么不用现成库：这台机器上没装 websockets / websocket-client，
而这个探针要做的事非常小 —— 握手一次、收发文本帧、把原始帧打出来。
自己拼 6 行帧头比 pip install 更可靠，也方便把握手失败的 HTTP 响应原样看到。

用法:
    python tools/wsprobe.py wss://<server>:8443/ws tok_xxx
    python tools/wsprobe.py wss://<server>:8443/ws tok_xxx --send '{"hello":1}'
    python tools/wsprobe.py ws://127.0.0.1:8787 tok --script tools/probe-script.json
"""

import base64
import json
import os
import socket
import ssl
import struct
import sys
import time
from urllib.parse import urlparse


def handshake(url: str, token: str, timeout: float = 12.0):
    u = urlparse(url)
    secure = u.scheme == "wss"
    host = u.hostname or ""
    port = u.port or (443 if secure else 80)
    path = u.path or "/"
    if u.query:
        path += "?" + u.query

    raw = socket.create_connection((host, port), timeout=timeout)
    if secure:
        ctx = ssl._create_unverified_context()
        sock = ctx.wrap_socket(raw, server_hostname=host)
    else:
        sock = raw

    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "User-Agent: EchoCast-probe/1\r\n"
    )
    if token:
        req += f"Authorization: Bearer {token}\r\n"
    req += "\r\n"
    sock.sendall(req.encode())

    # 读到空行为止，把整个响应头拿全 —— 失败时这些头就是唯一线索
    buf = b""
    sock.settimeout(timeout)
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf += chunk
    head, _, rest = buf.partition(b"\r\n\r\n")
    lines = head.decode("latin-1").split("\r\n")
    status = lines[0] if lines else "(no response)"
    headers = {}
    for ln in lines[1:]:
        if ":" in ln:
            k, v = ln.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    return sock, status, headers, rest


def frames(sock, rest: bytes):
    """从 socket 里逐帧吐出 (opcode, payload)。只处理文本/关闭/ping。"""
    buf = bytearray(rest)

    def need(n):
        while len(buf) < n:
            chunk = sock.recv(65536)
            if not chunk:
                raise ConnectionError("peer closed")
            buf.extend(chunk)

    while True:
        need(2)
        b0, b1 = buf[0], buf[1]
        op = b0 & 0x0F
        masked = b1 & 0x80
        ln = b1 & 0x7F
        idx = 2
        if ln == 126:
            need(4)
            ln = struct.unpack(">H", bytes(buf[2:4]))[0]
            idx = 4
        elif ln == 127:
            need(10)
            ln = struct.unpack(">Q", bytes(buf[2:10]))[0]
            idx = 10
        mask = b""
        if masked:
            need(idx + 4)
            mask = bytes(buf[idx:idx + 4])
            idx += 4
        need(idx + ln)
        payload = bytes(buf[idx:idx + ln])
        del buf[:idx + ln]
        if masked:
            payload = bytes(c ^ mask[i % 4] for i, c in enumerate(payload))
        yield op, payload


def send_text(sock, text: str):
    data = text.encode()
    head = bytearray([0x81])
    n = len(data)
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


def ts() -> str:
    return time.strftime("%H:%M:%S")


def main() -> int:
    args = [a for a in sys.argv[1:]]
    if len(args) < 1:
        print(__doc__)
        return 2
    url = args[0]
    token = args[1] if len(args) > 1 and not args[1].startswith("--") else ""

    script = []
    if "--script" in args:
        with open(args[args.index("--script") + 1], encoding="utf-8") as f:
            script = json.load(f)
    elif "--send" in args:
        script = [{"at": 0, "data": args[args.index("--send") + 1]}]

    print(f"[{ts()}] connect {url} token={'yes' if token else 'no'}")
    try:
        sock, status, headers, rest = handshake(url, token)
    except Exception as e:  # noqa: BLE001
        print(f"[{ts()}] HANDSHAKE FAILED: {type(e).__name__}: {e}")
        return 1

    print(f"[{ts()}] {status}")
    for k in ("upgrade", "connection", "sec-websocket-accept", "location", "server", "content-type"):
        if k in headers:
            print(f"        {k}: {headers[k]}")

    if "101" not in status:
        # 不是 101 就是失败。把 body 也读出来（通常是 nginx 的错误页或 JSON 报错）
        body = rest
        try:
            sock.settimeout(3)
            body += sock.recv(65536)
        except Exception:  # noqa: BLE001
            pass
        text = body.decode("utf-8", "replace").strip()
        print(f"[{ts()}] NOT UPGRADED, body: {text[:600]}")
        return 1

    # 握手成功。脚本按 at(秒) 排队发出，同时收帧；没有脚本就一直挂着收。
    start = time.time()
    sock.settimeout(1.0)
    pending = sorted(script, key=lambda x: x.get("at", 0))
    ttl = float(os.environ.get("PROBE_TTL", "25"))
    try:
        for op, payload in frames(sock, rest):
            now = time.time() - start
            while pending and pending[0].get("at", 0) <= now:
                item = pending.pop(0)
                send_text(sock, item["data"])
                print(f"[{ts()}] → {item['data'][:300]}")
            if op == 0x1:
                print(f"[{ts()}] ← {payload.decode('utf-8', 'replace')[:800]}")
            elif op == 0x8:
                code = struct.unpack(">H", payload[:2])[0] if len(payload) >= 2 else 0
                reason = payload[2:].decode("utf-8", "replace")
                print(f"[{ts()}] CLOSE code={code} reason={reason}")
                return 0
            elif op == 0x9:
                print(f"[{ts()}] ping")
            elif op == 0xA:
                print(f"[{ts()}] pong")
            else:
                # 0x2 = 二进制。服务端若用 msgpack/压缩帧，这里才看得见 ——
                # 不打印的话表现是「连上了但什么都没收到」，最容易被误判成没推。
                print(f"[{ts()}] op=0x{op:x} len={len(payload)} head={payload[:120]!r}")
            if now > ttl:
                print(f"[{ts()}] ttl {ttl}s reached, stop")
                return 0
    except socket.timeout:
        # 超时不算错：等不到帧也要把已收到的打出来
        for item in pending:
            send_text(sock, item["data"])
            print(f"[{ts()}] → {item['data'][:300]}")
        print(f"[{ts()}] idle {ttl}s, no more frames")
        return 0
    except Exception as e:  # noqa: BLE001
        print(f"[{ts()}] loop ended: {type(e).__name__}: {e}")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
