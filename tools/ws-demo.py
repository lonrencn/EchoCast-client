"""最小「带路由」的推送服务，用来在没有真实服务端时验证 EchoCast 的接收链路。

零依赖，只用标准库。它实现三件真实服务端也必须做的事：

1. **凭 token 认人** —— 握手时读 Authorization，反查 token 属于哪台终端；
2. **按 target 投递** —— 一个任务只发给命中的终端，不是所有人都收；
3. **回执记账** —— 收到 ack 就知道「谁看见了、谁接了」。

用法：
    python tools/ws-demo.py                  # 契约字段 + 路由演示（默认）
    python tools/ws-demo.py --card           # 只推服务端原始 card 形状，验兼容层
    python tools/ws-demo.py --plain          # 不推路由任务，只按连接推进度
    python tools/ws-demo.py --quiet          # 什么都不自动推，只等 HTTP 指令（服务端驱动）
    python tools/ws-demo.py 9000             # 换端口

客户端里填 `127.0.0.1:8787`，token 填下面 TERMINALS 里的任意一个：
    ect_demo_alpha   → term_alpha（书房 Windows，标签 办公）
    ect_demo_beta    → term_beta （渲染节点，标签 渲染）
    ect_demo_gamma   → term_gamma（笔记本，标签 办公）
token 留空 = 匿名终端，只能收到 broadcast 任务 —— 这条规则就是 fail-closed。

也可以直接用 HTTP 建任务（同一个端口）：
    curl -X POST http://127.0.0.1:8787/api/tasks ^
         -H "content-type: application/json" ^
         -d "{\"target\":{\"kind\":\"terminal\",\"id\":\"term_alpha\"},\"title\":\"只发给书房那台\"}"
    curl http://127.0.0.1:8787/api/terminals
"""

import base64
import hashlib
import json
import os
import socket
import struct
import sys
import threading
import time
from pathlib import Path

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

# 媒体样例目录与 MIME 表。/files/<name> 从这里取，basename 防目录穿越。
MEDIA_DIR = Path(__file__).resolve().parent / "media-sample"
MEDIA_MIME = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".webp": "image/webp",
    ".gif": "image/gif",
    ".mp3": "audio/mpeg",
    ".wav": "audio/wav",
    ".ogg": "audio/ogg",
    ".mp4": "video/mp4",
    ".webm": "video/webm",
    ".pdf": "application/pdf",
    ".csv": "text/csv",
}

# ---------------------------------------------------------------------------
# 终端注册表（真实服务端里这张表在数据库）
# ---------------------------------------------------------------------------

TERMINALS = {
    "ect_demo_alpha": {"id": "term_alpha", "alias": "书房 Windows", "labels": ["办公"]},
    "ect_demo_beta": {"id": "term_beta", "alias": "渲染节点", "labels": ["渲染"]},
    "ect_demo_gamma": {"id": "term_gamma", "alias": "笔记本", "labels": ["办公"]},
}

# terminal_id -> {"sock": socket, "send": callable, "alias": str, "labels": list}
LIVE: dict[str, dict] = {}
LOCK = threading.Lock()

# 投递记录：(task_id, terminal_id) -> state。真实服务端就是 deliveries 表。
DELIVERIES: dict[tuple[str, str], str] = {}

_seq = {"task": 0, "anon": 0}


def next_task_id() -> str:
    _seq["task"] += 1
    return f"task_{_seq['task']:03d}"


def terminal_by_token(token: str):
    for tok, t in TERMINALS.items():
        if tok == token:
            return t
    return None


def online_ids() -> list[str]:
    with LOCK:
        return list(LIVE.keys())


# ---------------------------------------------------------------------------
# 路由
# ---------------------------------------------------------------------------


def resolve(spec: dict) -> tuple[list[str], str]:
    """target → 目标终端列表。返回 (ids, 说明文字)。

    三种形态：
      {"kind":"terminal","id":"term_alpha"}   精确一台
      {"kind":"label","labels":["办公"]}      组内匹配
      {"kind":"broadcast"}                    所有在线
    """
    kind = (spec or {}).get("kind", "broadcast")
    online = online_ids()

    if kind == "terminal":
        tid = spec.get("id", "")
        return ([tid] if tid in online else []), f"terminal:{tid}"

    if kind == "label":
        want = set(spec.get("labels") or [])
        hit = [
            tid
            for tid in online
            for t in [live_meta(tid)]
            if want and want.issubset(set(t.get("labels") or []))
        ]
        return hit, f"label:{sorted(want)}"

    return online, "broadcast"


def live_meta(tid: str) -> dict:
    with LOCK:
        e = LIVE.get(tid)
        return {"alias": e["alias"], "labels": e["labels"]} if e else {}


def task_frame(task: dict) -> dict:
    return {"op": "task.deliver", "task": task}


def deliver(title: str, spec: dict, executor: dict | None = None) -> dict:
    """建任务并投递。返回给 HTTP 调用方看的摘要。"""
    task_id = next_task_id()
    task = {
        "id": task_id,
        "title": title,
        "executor": executor or {"name": "Agent Center", "kind": "agent"},
        "payload": {},
        "timestamp": now_ms(),
        "actions": [
            {"id": "accept", "label": "开始", "style": "primary"},
            {"id": "reject", "label": "跳过", "style": "ghost"},
        ],
    }

    targets, desc = resolve(spec)
    for tid in targets:
        DELIVERIES[(task_id, tid)] = "sent"
        with LOCK:
            entry = LIVE.get(tid)
        if entry:
            entry["send"](task_frame(task))

    print(f"[route] {task_id} «{title}» → {desc} · 投递 {targets or '（无在线目标）'}", flush=True)
    return {"task_id": task_id, "target": desc, "delivered": targets}


def push_notice(payload: dict) -> dict:
    """按服务端原始形状推一条 notice。这是「服务端驱动客户端」最直接的入口：

        POST /api/tasks
        {"target":{"kind":"terminal","id":"term_alpha"},
         "id":"t-100", "task":"合成第 4 段", "status":"running", "progress":0.62,
         "result":"合成第 3–4 段", "executor":{"name":"Soundao","node":"Edge TTS"}}

    id 必须自己给：不给就等于每条都是新通知，进度会刷成一列气泡。
    """
    spec = payload.get("target") or {"kind": "broadcast"}
    targets, desc = resolve(spec)

    body = {
        "id": payload.get("id") or f"push-{_seq['task']}-{int(time.time() * 1000) % 100000}",
        "task": payload.get("title") or payload.get("task"),
        "executor": payload.get("executor") or {"name": "Agent Center", "kind": "agent"},
        "status": payload.get("status") or "running",
        "result": payload.get("result") or "",
        "actions": payload.get("actions") or [],
        "timestamp": payload.get("timestamp") or now_ms(),
    }
    for k in ("topic", "channel", "title", "progress", "terminal", "offline_since",
              "media", "detail", "ask",
              "agent_name", "note_alias"):
        if payload.get(k) is not None:
            body[k] = payload[k]

    frame = {"op": "notice", "notice": body}
    for tid in targets:
        DELIVERIES[(body["id"], tid)] = "sent"
        with LOCK:
            entry = LIVE.get(tid)
        if entry:
            entry["send"](frame)

    prog = body.get("progress")
    print(
        f"[push] {body['id']} {body['status']}"
        f"{'' if prog is None else f' {float(prog):.0%}'}"
        f" «{body.get('task')}» → {desc} · 投递 {targets or '（无在线目标）'}",
        flush=True,
    )
    return {
        "notice_id": body["id"],
        "status": body["status"],
        "target": desc,
        "delivered": targets,
    }


# ---------------------------------------------------------------------------
# WS 协议
# ---------------------------------------------------------------------------


def accept_key(key: str) -> str:
    return base64.b64encode(hashlib.sha1((key + GUID).encode()).digest()).decode()


def build_frame(text: str) -> bytes:
    data = text.encode()
    n = len(data)
    head = b"\x81"
    if n < 126:
        head += bytes([n])
    elif n < 65536:
        head += bytes([126]) + struct.pack(">H", n)
    else:
        head += bytes([127]) + struct.pack(">Q", n)
    return head + data


def build_pong(payload: bytes) -> bytes:
    return b"\x8a" + bytes([len(payload)]) + payload


def read_frame(sock):
    head = sock.recv(2)
    if len(head) < 2:
        return None
    opcode = head[0] & 0x0F
    masked = head[1] & 0x80
    length = head[1] & 0x7F
    if length == 126:
        length = struct.unpack(">H", sock.recv(2))[0]
    elif length == 127:
        length = struct.unpack(">Q", sock.recv(8))[0]
    mask = sock.recv(4) if masked else b""
    payload = b""
    while len(payload) < length:
        chunk = sock.recv(length - len(payload))
        if not chunk:
            return None
        payload += chunk
    if masked:
        payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    return opcode, payload


def now_ms() -> int:
    return int(time.time() * 1000)


def notice(**kw) -> dict:
    base = {
        "id": "demo-1",
        "task": "合成《传承的力量》第 4 段",
        "executor": {"name": "Soundao", "node": "Edge TTS", "kind": "local"},
        "status": "running",
        "result": "",
        "actions": [],
        "timestamp": now_ms(),
        "channel": "soundao",
    }
    base.update(kw)
    return {k: v for k, v in base.items() if v is not None}


STEPS = [
    (0.05, "切分 6 个语段"),
    (0.24, "合成第 1–2 段"),
    (0.48, "合成第 3–4 段"),
    (0.71, "混流与响度归一"),
    (0.88, "写出 24bit/48kHz WAV"),
]


def card_frame(topic: str, task: str, result: str, source: str, **extra) -> dict:
    body = {
        "topic": topic,
        "text": f"{task} - {result}",
        "card": {
            "title": "任务执行中" if topic.endswith("running") else "任务完成",
            "source": source,
            "fields": [{"label": "任务", "value": task}],
        },
    }
    if not topic.endswith("running"):
        body["card"]["action"] = {"label": "查看结果", "url": "https://example.com/r/demo"}
    body.update(extra)
    return body


CARD_FINAL = "1 分 42 秒，响度 -16 LUFS"

# ---------------------------------------------------------------------------
# HTTP 请求读取（同端口，靠 Upgrade 头区分）
# ---------------------------------------------------------------------------


def read_head(sock):
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            return None, b""
        buf += chunk
    head, _, rest = buf.partition(b"\r\n\r\n")
    return head, rest


def parse_head(head: bytes) -> tuple[str, str, dict]:
    lines = head.decode("utf-8", "ignore").split("\r\n")
    parts = lines[0].split(" ") if lines else ["", ""]
    method, path = parts[0], parts[1] if len(parts) > 1 else "/"
    headers = {}
    for line in lines[1:]:
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    return method, path, headers


def read_body(sock, headers: dict, rest: bytes) -> bytes:
    try:
        length = int(headers.get("content-length", "0") or 0)
    except ValueError:
        length = 0
    body = rest
    while len(body) < length:
        chunk = sock.recv(length - len(body))
        if not chunk:
            break
        body += chunk
    return body[:length] if length else b""


def http_reply(sock, code: int, obj):
    text = {200: "OK", 400: "Bad Request", 401: "Unauthorized", 404: "Not Found"}.get(code, "OK")
    data = json.dumps(obj, ensure_ascii=False).encode()
    sock.sendall(
        (
            f"HTTP/1.1 {code} {text}\r\n"
            "Content-Type: application/json; charset=utf-8\r\n"
            f"Content-Length: {len(data)}\r\n"
            "Connection: close\r\n\r\n"
        ).encode()
        + data
    )


def handle_http(sock, head: bytes, rest: bytes):
    method, path, headers = parse_head(head)
    body = read_body(sock, headers, rest)
    try:
        payload = json.loads(body.decode("utf-8")) if body else {}
    except (json.JSONDecodeError, UnicodeDecodeError):
        payload = {}

    # 媒体样例文件：GET /files/<name>，从 tools/media-sample/ 里取。
    # 客户端下载走这条 —— 正好验证「媒体与 WS 同端口不同路径」也行。
    if path.startswith("/files/") and method == "GET":
        name = os.path.basename(path[len("/files/"):])
        f = MEDIA_DIR / name
        if not f.is_file():
            sock.sendall(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            return
        data = f.read_bytes()
        mime = MEDIA_MIME.get(f.suffix.lower(), "application/octet-stream")
        sock.sendall(
            (
                f"HTTP/1.1 200 OK\r\n"
                f"Content-Type: {mime}\r\n"
                f"Content-Length: {len(data)}\r\n"
                "Connection: close\r\n\r\n"
            ).encode()
            + data
        )
        return

    if path.startswith("/api/terminals") and method == "GET":
        out = []
        with LOCK:
            for tok, t in TERMINALS.items():
                out.append(
                    {
                        "id": t["id"],
                        "alias": t["alias"],
                        "labels": t["labels"],
                        "online": t["id"] in LIVE,
                        "token_prefix": tok[:12] + "…",
                    }
                )
        http_reply(sock, 200, {"terminals": out})
        return

    if path.startswith("/api/tasks") and method == "POST":
        title = payload.get("title") or payload.get("task")
        if not title:
            http_reply(sock, 400, {"error": "title 不能为空"})
            return
        # 带 status / progress 的走后一条路：直接推 notice。
        # 幂等：同一个 id 反复推 = 同一条通知在长进度，客户端按 id 合并，不会刷成一列。
        if payload.get("status") or payload.get("progress") is not None:
            res = push_notice(payload)
        else:
            res = deliver(title, payload.get("target") or {"kind": "broadcast"})
        http_reply(sock, 200, res)
        return

    http_reply(sock, 404, {"error": "未知路径", "hint": "GET /api/terminals · POST /api/tasks"})


# ---------------------------------------------------------------------------
# WS 会话
# ---------------------------------------------------------------------------


def handle_ws(sock, addr, head: bytes):
    method, path, headers = parse_head(head)
    key = headers.get("sec-websocket-key", "")
    if not key:
        sock.close()
        return

    auth = headers.get("authorization", "")
    token = auth[7:].strip() if auth.lower().startswith("bearer ") else ""
    term = terminal_by_token(token)

    # 填了 token 但不认识 → 401。客户端会据此停止重连，这正是想要的行为：
    # token 错了无限重试只会把服务端日志刷满。
    if token and not term:
        sock.sendall(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        print(f"[ws] {addr[0]}:{addr[1]} token 无法识别，已拒绝")
        sock.close()
        return

    if term:
        tid, alias, labels = term["id"], term["alias"], list(term["labels"])
    else:
        # 匿名：能连，但只会收到 broadcast。这就是「token 决定你是谁」的体现。
        _seq["anon"] += 1
        tid = f"term_anon{_seq['anon']}"
        alias, labels = "匿名终端", []

    sock.sendall(
        (
            "HTTP/1.1 101 Switching Protocols\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Accept: {accept_key(key)}\r\n\r\n"
        ).encode()
    )
    print(f"[ws] {addr[0]}:{addr[1]} 已连接 → {tid}（{alias}）")

    stop = threading.Event()
    wlock = threading.Lock()

    def send_raw(obj: dict):
        with wlock:
            try:
                sock.sendall(build_frame(json.dumps(obj, ensure_ascii=False)))
            except OSError:
                stop.set()

    def send_notice(obj: dict):
        """通知统一包 {op:"notice"} 信封 —— 服务端定的就是这个。"""
        send_raw({"op": "notice", "notice": obj})

    with LOCK:
        LIVE[tid] = {"sock": sock, "send": send_raw, "alias": alias, "labels": labels}

    # 对号：告诉客户端「你被我认成了谁」。真实服务端在这一步绑定连接与终端。
    send_raw(
        {
            "op": "hello_ack",
            "terminal": {"id": tid, "alias": alias, "labels": labels},
            "server_time": now_ms(),
            "protocol": "1.1",
        }
    )

    def push():
        """--plain / --card 模式下，按连接推自己的进度演示。"""
        time.sleep(0.6)
        round_no = 0
        while not stop.is_set():
            if mode == "card":
                task = f"生成 Q3 市场趋势分析 #{round_no + 1}"
                for p, note in STEPS:
                    if stop.is_set():
                        return
                    send_notice(card_frame("task.running", task, note, "Research Agent",
                                           progress=int(p * 100)))
                    print(f"[ws] card {p:.0%} · {note}", flush=True)
                    time.sleep(1.4)
                if stop.is_set():
                    return
                send_notice(card_frame("task.completed", task, CARD_FINAL, "Research Agent"))
                print("[ws] card completed（无 id/status/timestamp）", flush=True)
            else:
                rid = f"demo-{round_no}"
                for p, note in STEPS:
                    if stop.is_set():
                        return
                    send_notice(notice(id=rid, progress=p, result=note))
                    print(f"[ws] {rid} running {p:.0%} · {note}", flush=True)
                    time.sleep(1.4)
                if stop.is_set():
                    return
                send_notice(
                    notice(
                        id=rid,
                        status="success",
                        result="1 分 42 秒，响度 -16 LUFS",
                        actions=[{"id": "open", "label": "试听", "style": "primary"}],
                        progress=None,
                        media=[
                            {"kind": "image", "url": f"http://127.0.0.1:{PORT}/files/sample-cover.webp",
                             "name": "节目封面.webp"},
                            {"kind": "audio", "url": f"http://127.0.0.1:{PORT}/files/sample-tone.wav",
                             "name": "试听片段.wav"},
                            {"kind": "file", "url": f"http://127.0.0.1:{PORT}/files/sample-script.csv",
                             "name": "台本.csv"},
                        ],
                    )
                )
                print(f"[ws] {rid} success", flush=True)
            round_no += 1
            time.sleep(7)

    if mode in ("plain", "card"):
        threading.Thread(target=push, daemon=True).start()

    try:
        while not stop.is_set():
            got = read_frame(sock)
            if got is None:
                break
            opcode, payload = got
            if opcode == 0x8:
                break
            if opcode == 0x9:
                sock.sendall(build_pong(payload))
                continue
            if opcode != 0x1:
                continue

            try:
                up = json.loads(payload.decode("utf-8", "ignore"))
            except json.JSONDecodeError:
                continue
            op = up.get("op")

            if op == "hello":
                dev = up.get("device") or {}
                want_alias = (up.get("alias") or "").strip()
                want_labels = up.get("labels") or []
                with LOCK:
                    e = LIVE.get(tid)
                    if e:
                        # 只在服务端没指定时采纳客户端建议；已有别名就不覆盖
                        if want_alias and not TERMINALS.get(token, {}).get("alias"):
                            e["alias"] = want_alias
                        if want_labels and tid.startswith("term_anon"):
                            e["labels"] = want_labels
                print(
                    f"[ws] hello ← {tid} alias={want_alias!r} labels={want_labels} "
                    f"os={dev.get('os')} v={dev.get('app_version')} device={str(dev.get('device_id'))[:8]}…"
                )
                with LOCK:
                    meta = LIVE.get(tid) or {}
                send_raw(
                    {
                        "op": "hello_ack",
                        "terminal": {
                            "id": tid,
                            "alias": meta.get("alias", alias),
                            "labels": meta.get("labels", labels),
                        },
                        "server_time": now_ms(),
                        "protocol": "1.1",
                    }
                )

            elif op == "subscribe":
                topics = up.get("topics") or []
                print(f"[ws] 订阅 {len(topics)} 个 topic（过滤器，不参与路由）")
                send_raw({"op": "subscribed", "topics": topics})

            elif op == "ack":
                task_id = up.get("task_id", "")
                state = up.get("state", "")
                if task_id:
                    DELIVERIES[(task_id, tid)] = state
                print(f"[route] ack ← {tid} {task_id} = {state}")

                # 抢单：label/broadcast 建的任务，第一个 ack:accepted 的接单，
                # 其余终端收撤回。不收的话三台机器会同时显示同一个任务。
                if state == "accepted":
                    for (t_id, other), st in list(DELIVERIES.items()):
                        if t_id == task_id and other != tid and st in ("sent", "seen"):
                            DELIVERIES[(t_id, other)] = "superseded"
                            with LOCK:
                                oe = LIVE.get(other)
                            if oe:
                                oe["send"]({"op": "task.revoke", "task_id": task_id,
                                            "reason": f"{tid} 先接了"})
                                print(f"[route] 撤回 → {other}（{tid} 已接单）")

            elif op == "action":
                print(
                    f"[ws] 动作 ← {up.get('notice_id')} / {up.get('action_id')} "
                    f"payload={up.get('payload')}"
                )
            elif op == "pong":
                pass
            else:
                print(f"[ws] 未知上行帧: {up}")

    except OSError:
        pass
    finally:
        stop.set()
        with LOCK:
            LIVE.pop(tid, None)
        sock.close()
        print(f"[ws] {addr[0]}:{addr[1]} 断开（{tid}）")


def handle_conn(sock, addr):
    sock.settimeout(60)
    try:
        head, rest = read_head(sock)
        if head is None:
            sock.close()
            return
        if b"upgrade: websocket" in head.lower():
            handle_ws(sock, addr, head)
        else:
            handle_http(sock, head, rest)
    except OSError:
        try:
            sock.close()
        except OSError:
            pass


# ---------------------------------------------------------------------------
# 路由演示：每 9 秒建一个任务，目标轮换
# ---------------------------------------------------------------------------

DEMO_TARGETS = [
    ({"kind": "terminal", "id": "term_alpha"}, "只发给书房那台"),
    ({"kind": "terminal", "id": "term_beta"}, "只发给渲染节点"),
    ({"kind": "label", "labels": ["办公"]}, "发给标签「办公」的机器"),
    ({"kind": "broadcast"}, "广播给所有在线终端"),
]


def revoke(task_id: str, reason: str, only: list[str] | None = None):
    """撤回一条任务。抢单场景和演示轮换都要用 ——
    不撤回的话，一台终端会攒一屏「待处理」，那是待办列表，不是气泡。"""
    for (tid_task, tid), st in list(DELIVERIES.items()):
        if tid_task != task_id or st in ("revoked", "superseded"):
            continue
        if only is not None and tid not in only:
            continue
        DELIVERIES[(tid_task, tid)] = "revoked"
        with LOCK:
            e = LIVE.get(tid)
        if e:
            e["send"]({"op": "task.revoke", "task_id": task_id, "reason": reason})
            print(f"[route] 撤回 {task_id} → {tid}（{reason}）", flush=True)


def route_loop():
    time.sleep(2.0)
    i = 0
    last = None
    while True:
        # 上一轮还没处理的先撤掉：演示循环每 9 秒一条，不撤会堆成待办列表。
        if last:
            revoke(last, "演示轮换")
        spec, title = DEMO_TARGETS[i % len(DEMO_TARGETS)]
        res = deliver(title, spec, {"name": "Agent Center", "kind": "agent"})
        last = res["task_id"]
        i += 1
        time.sleep(9)


def main():
    global mode, PORT

    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", line_buffering=True)

    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--card" in sys.argv:
        mode = "card"
    elif "--plain" in sys.argv:
        mode = "plain"
    elif "--quiet" in sys.argv:
        mode = "quiet"
    else:
        mode = "route"
    port = int(args[0]) if args else 8787
    PORT = port

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", port))
    srv.listen(8)

    label = {
        "route": "路由演示（按 terminal / label / broadcast 投递）",
        "card": "服务端原始 card 形状（缺 id/status/timestamp）",
        "plain": "按连接推契约字段",
        "quiet": "只等 HTTP 指令，不自动推（服务端驱动用这个）",
    }[mode]
    print(f"[ws] 监听 ws://127.0.0.1:{port} · {label}")
    print("[ws] 建任务: curl -X POST http://127.0.0.1:%d/api/tasks "
          "-H \"content-type: application/json\" "
          "-d '{\"target\":{\"kind\":\"terminal\",\"id\":\"term_alpha\"},\"title\":\"测试\"}'" % port)
    print("[ws] 终端表: curl http://127.0.0.1:%d/api/terminals" % port)

    if mode == "route":
        threading.Thread(target=route_loop, daemon=True).start()

    while True:
        conn, addr = srv.accept()
        threading.Thread(target=handle_conn, args=(conn, addr), daemon=True).start()


mode = "route"

if __name__ == "__main__":
    main()
