#!/usr/bin/env python3
"""本地握手抓包：把客户端真实发出的 WS 握手请求头原样打印。

用途：服务端只给「某种握手」推帧，而探针一直被晾着。
与其猜 header，不如让客户端连到本地，把它发出的东西逐字节记下来，
再照着复制到探针里 —— 这样能确定差异到底在哪一行。
"""

import base64
import hashlib
import socket
import sys
import threading
import time

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 9797
TTL = float(sys.argv[2]) if len(sys.argv) > 2 else 40.0
GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def handle(conn, addr):
    conn.settimeout(TTL)
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = conn.recv(4096)
            if not chunk:
                return
            buf += chunk
        head, _, rest = buf.partition(b"\r\n\r\n")
        print("=" * 70, flush=True)
        print(f"[{time.strftime('%H:%M:%S')}] 来自 {addr}", flush=True)
        print(head.decode("latin-1"), flush=True)

        key = ""
        for ln in head.decode("latin-1").split("\r\n"):
            if ln.lower().startswith("sec-websocket-key:"):
                key = ln.split(":", 1)[1].strip()
        accept = base64.b64encode(hashlib.sha1((key + GUID).encode()).digest()).decode()
        conn.sendall(
            (
                "HTTP/1.1 101 Switching Protocols\r\n"
                "Upgrade: websocket\r\n"
                "Connection: Upgrade\r\n"
                f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
            ).encode()
        )
        # 握手后把客户端发的帧也打出来（hello / subscribe），它才是最有价值的部分
        data = bytearray(rest)
        end = time.time() + TTL
        while time.time() < end:
            try:
                chunk = conn.recv(65536)
                if not chunk:
                    break
                data.extend(chunk)
            except socket.timeout:
                break
            # 只做最糙的解析：把可见文本片段抽出来
            txt = bytes(c for c in data if 32 <= c < 127 or c > 127)
            try:
                decoded = txt.decode("utf-8", "replace")
            except Exception:
                decoded = repr(txt)
            print(f"  [recv {len(data)}B] {decoded[:600]}", flush=True)
            data.clear()
    except Exception as e:  # noqa: BLE001
        print(f"  !! {type(e).__name__}: {e}", flush=True)
    finally:
        try:
            conn.close()
        except Exception:
            pass


def main():
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", PORT))
    srv.listen(8)
    srv.settimeout(TTL)
    print(f"listening :{PORT} for {TTL:.0f}s", flush=True)
    end = time.time() + TTL
    while time.time() < end:
        try:
            c, a = srv.accept()
        except socket.timeout:
            break
        threading.Thread(target=handle, args=(c, a), daemon=True).start()
    time.sleep(1)


if __name__ == "__main__":
    main()
