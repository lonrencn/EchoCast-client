# 终端身份与任务路由方案

> 目标：服务端能创建任务，并指定投递给**某一台**终端（而不是所有终端都收到）。
> 每台终端一个独立 token，服务端凭 token 认人，凭 target 选人。
>
> 状态：方案待评审 · 客户端侧基础能力已实现 · 服务端侧待实现

---

## 1. 先把三个东西分开

现在这三件事是混在一起的，混着做必然出错：

| 概念 | 是什么 | 谁产生 | 作用 |
|---|---|---|---|
| **终端**（terminal） | 一台装了 EchoCast 的机器 | 服务端签发身份 | 决定「发给谁」 |
| **任务**（task） | 一件要人处理的事 | 服务端创建 | 决定「发什么」 |
| **通知**（notice） | 任务状态的一次快照 | 服务端推、客户端渲染 | 决定「怎么显示」 |

一条任务会产出多条通知（running 5 次 + 终态 1 次），但**任务只有一个目标和一条投递记录**。
所以路由是任务级的，不是通知级的 —— 否则同一条任务的 6 帧可能被投到不同终端，
用户会看到「进度条在 A 机器走，结果在 B 机器弹」。

---

## 2. 标识与凭证

| 名称 | 形态 | 稳定性 | 存在哪 |
|---|---|---|---|
| `terminal_id` | `term_` + 16 hex | 永久不变 | 服务端 `terminals.id`，客户端缓存 |
| `token` | `ect_` + 48 hex（24 字节） | 可轮换 | 服务端存**哈希**，客户端存**加密存储** |
| `device_id` | 随机 UUID v4 | 重装应用不变，重装系统变 | 客户端 `$APPDATA/device.json` |
| `alias` | `书房 Windows` | 用户可改 | 服务端权威，客户端可改并上报 |

`device_id` 是首次启动生成的随机 UUID，**不是硬件指纹**。理由：主板序列号/MAC 在
虚拟机上不稳定（换块网卡就变成一台新机器），而取这些信息在部分系统上还要额外权限。
真需要防重装时，服务端拿 IP + 首次接入时间人工核对更省事。

### 2.1 为什么 token 必须与终端一对一

现在 token 是「入网令牌」，谁拿到都能连，服务端连接建立后**不知道对面是谁**。
于是只能广播，客户端再自己筛 —— 这不是路由，是碰运气。

一对一之后有三个直接好处：

1. **服务端能路由**：连接一建立就知道 `terminal_id`，推送时按目标过滤。
2. **泄漏可控**：一个 token 泄漏只影响一台终端，吊销这一条即可，其余终端不受影响。
3. **可审计**：`deliveries` 表能记清「这条任务是谁收的、什么时候收的」。

### 2.2 反例：不要在 payload 里自报身份

```
❌ { "op":"hello", "terminal_id":"term_7f3a..." }   // 客户端说了算
✅ Authorization: Bearer ect_xxx                     // 服务端说了算
```

任何由客户端填写的 `terminal_id` 都是可以随便写的。做过一次「订阅 `terminal:<id>`」
的方案就会永久留下一个洞：任何终端只要知道别人的 id 就能偷听别人的任务。
**身份必须来自凭证，不能来自报文。**

---

## 3. 服务端数据模型

```sql
-- 终端注册表。token 只存哈希，明文仅在签发时返回一次。
CREATE TABLE terminals (
  id             TEXT PRIMARY KEY,           -- 'term_7f3a1c...'
  alias          TEXT NOT NULL,              -- 用户可见名，可改
  token_hash     TEXT NOT NULL UNIQUE,       -- sha256(token)，查询用
  token_prefix   TEXT NOT NULL,              -- 'ect_3f9a'，只用于列表展示与审计
  labels         TEXT NOT NULL DEFAULT '[]', -- JSON 数组：["办公","win11"]
  device_fp      TEXT,                       -- 重装识别用，可空
  app_version    TEXT,
  os             TEXT,                       -- 'windows' / 'darwin' / 'linux'
  status         TEXT NOT NULL DEFAULT 'offline', -- online | offline | busy
  last_seen      INTEGER,                    -- 毫秒
  created_at     INTEGER NOT NULL,
  revoked_at     INTEGER                     -- 非空即已吊销
);

-- 任务。target 决定投给谁。
CREATE TABLE tasks (
  id          TEXT PRIMARY KEY,              -- 'task_...'
  title       TEXT NOT NULL,
  payload     TEXT,                          -- 任务体（JSON，内容由业务定）
  target_kind TEXT NOT NULL,                 -- terminal | label | broadcast
  target_val  TEXT NOT NULL,                 -- terminal_id / JSON 数组 / ''
  strategy    TEXT NOT NULL DEFAULT 'first', -- first | all
  state       TEXT NOT NULL DEFAULT 'pending', -- pending | running | done | failed | expired
  created_by  TEXT,
  created_at  INTEGER NOT NULL,
  expires_at  INTEGER
);

-- 投递明细。一个任务 N 个目标就有 N 行，各自独立推进。
CREATE TABLE deliveries (
  task_id     TEXT NOT NULL,
  terminal_id TEXT NOT NULL,
  state       TEXT NOT NULL DEFAULT 'pending', -- pending | sent | acked | done | rejected | expired
  attempts    INTEGER NOT NULL DEFAULT 0,
  sent_at     INTEGER,
  acked_at    INTEGER,
  fail_reason TEXT,
  PRIMARY KEY (task_id, terminal_id)
);
```

`deliveries` 是这套方案的关键。没有它，「任务发出去了吗」只能靠日志猜。

---

## 4. 注册与鉴权

### 4.1 首次接入（两种入口，都保留）

**甲：管理员在服务端建终端 → 拿 token 手填**

服务端控制台「新建终端」→ 填别名和标签 → 返回一次性 token → 用户在客户端齿轮里粘贴。
简单，适合现在 3–5 台机器的阶段。

**乙：激活码（P1）**

服务端生成 8 位一次性激活码（10 分钟有效），用户输入后客户端自动换取 token。
比手填 48 位 hex 好得多，也让「加一台新机器」不需要管理员在旁边。

```
POST /api/terminals/activate
{ "code": "7F3A-9C21", "alias": "书房 Windows", "device_fp": "…", "os": "windows", "app_version": "0.1.0" }

→ 200 { "terminal": { "id": "term_7f3a…", "alias": "书房 Windows", "labels": ["办公"] },
        "token": "ect_…" }   // 只此一次返回
→ 409 { "error": "code_used" }
```

### 4.2 连接时序

```
客户端                                    服务端
  │  GET /ws                               │
  │  Authorization: Bearer ect_xxx         │
  │ ─────────────────────────────────────► │ 1. sha256(token) 查 terminals
  │                                        │    查不到 → 401
  │                                        │    revoked_at 非空 → 403
  │ ◄────────────── 101 Switching ──────── │ 2. 绑定连接 ↔ terminal_id
  │                                        │    status = online
  │  {op:"hello", device:{…}, labels:[…]}  │
  │ ─────────────────────────────────────► │ 3. 更新 alias/labels/版本
  │ ◄── {op:"hello_ack", terminal:{…}} ─── │    下发服务端时间与协议版本
  │  {op:"subscribe", topics:[…]}          │
  │ ─────────────────────────────────────► │ 4. 订阅 = 事件类型过滤（不承担路由）
  │ ◄── {op:"subscribed", …} ───────────── │
  │                                        │ 5. 补投离线期间累计的任务
  │ ◄── {op:"task.deliver", task:{…}} ──── │
  │  {op:"ack", task_id, state:"seen"}     │
  │ ─────────────────────────────────────► │
  │  … 任务执行过程推 notice …              │
```

### 4.3 关闭码约定（客户端必须能区分）

| code | 含义 | 客户端行为 |
|---|---|---|
| 4001 | token 无效 | **停止重连**，UI 提示「token 无效，请重新填写」 |
| 4003 | token 已吊销 | **停止重连**，提示「该终端已被管理员吊销」 |
| 4029 | 同一终端被别的实例占用 | 停止重连并提示，避免两台机器互踢 |
| 1000 / 1005 | 正常关闭 | 正常重连 |
| 其他 | 网络/服务端故障 | 正常指数退避重连 |

握手阶段被拒也一样处理：HTTP **401 / 403 直接判为配置错误**，不再重试。

这一条不是细节。现在客户端对所有错误一视同仁地无限重连，token 填错时会
每 30 秒撞一次服务端，日志里全是 401，真正的问题会被淹掉。
（已实现并有测试覆盖：`conn::tests::auth_close_codes_stop_retrying`）

---

## 5. 帧协议增量

现有 `{op:"notice"}`、`{op:"subscribe"}`、`{op:"action"}` 全部保留，新增四类：

### 5.1 `hello` / `hello_ack`

```jsonc
// 上行，连上后立刻发（在 subscribe 之前）
{ "op": "hello",
  "protocol": "1",
  "alias": "书房 Windows",
  "labels": ["办公", "win11"],
  "device": { "device_id": "7a0e3ee5-…", "os": "windows",
              "hostname": "LONREN-HOME", "app_version": "0.1.0" } }

// 下行
{ "op": "hello_ack",
  "terminal": { "id": "term_7f3a…", "alias": "书房 Windows", "labels": ["办公"] },
  "server_time": 1790000000000,
  "protocol": "1.1" }
```

`labels` 以服务端为准：客户端上报的只是「建议」，管理员在服务端改的会覆盖回来。
已实测：客户端报 `办公,win11`，服务端只留 `办公`，界面显示的就是 `办公`。

### 5.2 `task.deliver`（任务下发）

```jsonc
{ "op": "task.deliver",
  "task": {
    "id": "task_9f2c",
    "title": "合成《传承的力量》第 4 段",
    "executor": { "name": "Soundao", "node": "Edge TTS", "kind": "local" },
    "payload": { "text_path": "…", "voice": "zh-CN-Yunxi" },
    "expires_at": 1790000600000,
    "actions": [
      { "id": "accept", "label": "开始", "style": "primary" },
      { "id": "reject", "label": "跳过", "style": "ghost" }
    ]
  } }
```

### 5.3 `ack`（回执，上行）

```jsonc
{ "op": "ack", "task_id": "task_9f2c", "state": "seen" }      // seen | accepted | rejected | done
{ "op": "ack", "task_id": "task_9f2c", "state": "rejected", "reason": "正在跑别的任务" }
```

`seen` 由 **Rust 侧收到投递后立刻自动回**，不经过 UI、不等用户点击。
放在前端回的话，窗口还没加载完就会漏掉一批回执。服务端靠它判断
「人到底看见了没有」，这是唯一能自动拿到的信号。

### 5.4 `presence`（在线状态，下行）

```jsonc
{ "op": "presence", "terminal_id": "term_7f3a", "status": "busy", "load": 3 }
```

用于多端选主：管理页能看到哪台机器空闲。

---

## 6. 路由规则

`target_kind` 三种：

| kind | target_val | 语义 | 典型场景 |
|---|---|---|---|
| `terminal` | `term_7f3a…` | 精确一台 | 「这条只给渲染机」 |
| `label` | `["办公"]` | 组内匹配，`match` 默认 all | 「投给所有办公机」 |
| `broadcast` | `''` | 所有在线终端 | 「全站维护通知」 |

`label` 路由的 `strategy`：

- `first`（默认）：先 `ack:seen` 的终端接单，其余终端的投递置为 `superseded` 并推一帧撤回
  `{op:"task.revoke", task_id}`，气泡收起。**抢单场景必须实现撤回**，否则三台机器都弹一个卡片。
- `all`：每台都独立执行（如「所有机器刷新配置」）。

### 6.1 一个容易忽略的约束

`broadcast` + `first` 组合会让「谁先看到谁做」，这个语义本身很怪。
建议：`broadcast` 强制 `all`，`first` 只允许配在 `label` / `terminal` 上。
服务端建任务时校验，别指望客户端兜底。

---

## 7. 投递状态机

```
pending ──sending──> sent ──ack:seen──> acked ──ack:done──> done
   │                  │                    │
   │                  │                    └──ack:rejected──> rejected
   │                  └──超时(30s)──> pending（重试，attempts+1，上限 3）
   └──过期(expires_at)──> expired
```

规则：

1. **离线不丢**：终端离线时投递停在 `pending`，上线后 `hello_ack` 之后立刻补投。
   补投时在 `task` 里加 `"offline_since": 1790000000`，客户端 UI 提示「离线期间的任务」。
2. **重试上限 3 次**：3 次没 `seen` 就置 `expired` 并推一条失败通知给创建者，
   而不是无限重试 —— 一台关机的机器能让队列涨到几万条。
3. **`seen` 的判定**：客户端收到 `task.deliver` 并完成渲染后立刻回，**不要**等用户交互。
4. **`done` 由谁回**：v1 由终端回（任务在本地跑完）；v2 如果任务其实在服务端跑、
   终端只负责展示，那就由服务端自己置 `done`，终端不用回。

第 4 点是 v1 必须跟服务端确认的一件事：**任务是在终端上执行，还是只在终端上展示？**
两种的 `ack` 责任方完全不同。当前 EchoCast 只是通知展示器，所以我建议 v1 按
「展示 + 回执」实现，`done` 由服务端置。

---

## 8. 安全

| 项 | 做法 | 为什么 |
|---|---|---|
| token 存储（服务端） | 只存 `sha256`，明文仅签发时返回一次 | 数据库备份泄漏 ≠ 全部终端被接管 |
| token 存储（客户端） | Windows Credential Manager（DPAPI） | 明文 json 会被任何进程读走，且会进云同步盘 |
| token 展示 | 列表只显示 `ect_3f9a…` + 后 4 位 | 截图发给别人时不会连带泄漏 |
| 轮换 | 新旧 token 同时有效 10 分钟 | 换 token 期间不掉线 |
| 吊销 | 置 `revoked_at`，并主动 close 该终端的连接 | 只标记不踢，那台机器会一直在线 |
| 日志 | token 一律打码 | 服务端日志常被贴到群里排障 |

**客户端侧现状**：`settings.json` 里 token 是明文。这在开发期可接受，
正式发版前要换成系统凭据存储 —— 已列入待办。

---

## 9. 客户端改动清单

| 文件 | 改动 | 状态 |
|---|---|---|
| `src-tauri/src/device.rs` | 新增。生成并持久化 `device_id`，采集 hostname / os / app 版本 | ✅ 已实现 |
| `src-tauri/src/settings.rs` | 新增 `terminal_alias` / `terminal_labels` / `terminal_id`（缓存）+ `label_list()` | ✅ 已实现 |
| `src-tauri/src/conn.rs` | 连上后先发 `hello`；识别 `hello_ack` / `task.deliver` / `task.revoke` / `presence` | ✅ 已实现 |
| `src-tauri/src/conn.rs` | 收到 `task.deliver` 自动回 `ack:seen`；`ConnStatus::Fatal` + 4001/4003/4029 停止重连 | ✅ 已实现 |
| `src-tauri/src/conn.rs` | 新增命令 `conn_send_ack`（accepted / rejected / done）、`terminal_get` | ✅ 已实现 |
| `src-tauri/src/wire.rs` | `RawNotice` 支持 `title`（投递用）、`terminal`、`offline_since` | ✅ 已实现 |
| `src/types/notice.ts` | `TaskNotice` 增加 `terminal` / `offline_since` | ✅ 已实现 |
| `src/settings/useSettings.ts` | `terminal` 状态、`ec:terminal` 订阅、`fatal` 状态、`sendAck()` | ✅ 已实现 |
| `src/components/SettingsPanel.vue` | 「本机身份」区块：终端 id、设备 id、服务端下发的标签、别名/标签输入 | ✅ 已实现 |
| `src/feed/notices.ts` | 监听 `ec:revoke` 收掉被撤回的气泡 | ✅ 已实现 |
| `tools/ws-demo.py` | 终端注册表 + token 鉴权 + 三种 target 路由 + `POST /api/tasks` + `GET /api/terminals` | ✅ 已实现 |

客户端**不需要**知道路由规则。它只做三件事：报身份、收发给我的、回执。
路由逻辑一旦漏到客户端，就意味着客户端可以绕过它 —— 那就不是权限了。

---

## 10. 服务端改动清单

按依赖顺序：

1. `terminals` / `tasks` / `deliveries` 三张表 + 迁移。
2. `POST /api/terminals`（建终端，返回一次性 token）、`DELETE /api/terminals/:id`（吊销）。
3. WS 握手：`sha256(token)` 查表 → 绑定 `terminal_id`；失败 401/403 + 关闭码。
4. 连接注册表：`HashMap<terminal_id, Sender>`，断开时置 offline 并记录 `last_seen`。
5. `subscribe` 语义改为**事件类型过滤**，与路由正交（见 §6 注释）。
6. `POST /api/tasks`：建任务 → 解析 target → 生成 deliveries → 在线立即推、离线排队。
7. 补投：`hello_ack` 之后查 `deliveries where state='pending' and task 未过期`。
8. 回执处理：更新 deliveries，`first` 策略下触发 supresede + `task.revoke`。
9. 超时扫描：每分钟扫一次 `sent` 超过 30s 的，重试或置 expired。

---

## 11. topic 与 target 的关系（最容易做错的地方）

现在 topic 被当成广播频道用：客户端订阅 `task.completed`，服务端就往所有订阅者推。
加上路由之后，两者必须正交：

```
topic  = 事件类型（what）   → 客户端想知道哪类事，客户端自己订
target = 投递对象（who）    → 服务端决定发给谁，客户端无权干涉
```

判定顺序：**先按 target 算出目标终端集合，再检查每个终端是否订阅了该 topic。**
不是先广播再过滤 —— 那样每条通知都要遍历所有连接。

对客户端的兼容影响：现有 11 个 topic 全部保留，语义不变，只是从
「频道」降级为「过滤器」。**老版本客户端即使不理解 target，仍能正常收到属于它的通知**，
因为它们本来就只连自己那条连接。

---

## 12. 验收用例

| # | 场景 | 期望 | 实测 |
|---|---|---|---|
| 1 | 建任务 target=`terminal:term_beta` | 只有 beta 弹，其他无感 | ✅ `task_034` 只投给 `term_beta`，alpha/gamma 无帧 |
| 2 | target=`label:["办公"]`，两台办公机在线 | 两台都收到 | ✅ `task_035` 投给 `term_alpha` + `term_gamma`，beta 未收到 |
| 3 | target=`terminal:term_alpha` | 只有 alpha 收到 | ✅ `task_036` 仅 alpha，两个探针日志无此任务 |
| 4 | `broadcast` | 所有在线终端 | ✅ `task_028` 三台同时收到并各自 ack |
| 5 | 用 A 的 token 连，收到 B 的任务 | **不应发生** | ✅ 服务端按连接绑定路由，不看报文 |
| 6 | 吊销 token 并踢线 | 断开且不再重连 | 待服务端实现（客户端侧 4003 已实现） |
| 7 | token 填错 | 401，显示「token 无效」，**不无限重连** | ✅ 握手 401 判为 fatal；单测覆盖 |
| 8 | 同任务 6 帧进度 | 落同一终端、同一条气泡 | ✅ 已有实现（同 id 覆盖） |

实测用的三台终端：真实桌面客户端 `term_alpha`（token `ect_demo_alpha`），
加两个探针 `term_beta` / `term_gamma`。服务端是 `tools/ws-demo.py`，
建任务走 `POST /api/tasks`，回执看服务端日志的 `ack ←`。

复现：

```bash
python tools/ws-demo.py                  # 终端表 + 路由 + 建任务接口
curl http://127.0.0.1:8787/api/terminals
curl -X POST http://127.0.0.1:8787/api/tasks -H "content-type: application/json" \
     -d '{"target":{"kind":"label","labels":["办公"]},"title":"给办公组的机器"}'
```

---

## 13. 分期

**P0（本次，能跑通路由）**
终端表 + 一对一 token + 握手绑定 + `hello`/`hello_ack` + `task.deliver` + `ack:seen`
+ 按 `terminal` 定向 + 离线补投。

**P1（可运维）**
激活码、终端管理页（在线状态/吊销/改标签）、label 路由、`first` 抢单与撤回、token 轮换。

**P2（真执行）**
终端侧任务执行器、`presence` 负载上报、按负载选机、任务结果文件回传。

---

## 附：与总台的对接话术

> 我这边的改动是「报身份 + 回执」，路由完全放你那边。
> 你只需要保证一件事：**连接建立时用 token 反查终端，之后这条连接只能收到 target 命中它的任务。**
> 客户端不会、也不该在报文里自报 terminal_id —— 那等于没有权限。
>
> 最小实现是三张表（terminals / tasks / deliveries）加一个 `HashMap<terminal_id, Sender>`。
> 如果 P0 只想先做定向，label 路由和抢单可以后放，但 `deliveries` 表建议一次到位，
> 否则「任务发没发出去」只能翻日志。

---

## 14. 对接状态（2026-10-31 21:30，真服务端首轮联调）

入口已从「不通」变成「通」：`wss://<bridge 服务端地址>/ws`，
net-token 到手，压测循环 6 个 agent 在广播。以下是**真机窗口 + 真实服务端**
跑出来的结论，不是模拟。

### 14.1 通过项

| 项 | 实测 |
|---|---|
| 握手 | `101 Switching Protocols`（openresty 前置） |
| token 校验 | 无 token / 错 token → `4001 unauthorized`；客户端据此判致命、**停止重连** |
| 订阅 | `subscribe` 后 47ms 收到 `{"op":"subscribed","topics":[…]}`，9 个 topic 原样回显 |
| 帧格式 | 符合 v1.1；`{op:"notice",notice:{id,topic,status,progress,timestamp,ts,task,result,executor,actions,card}}` |
| 合并 | 434s / 38 任务 / 207 帧，每任务 5–7 帧，同 id 原地更新（22%→45%→65%→81%） |
| 终态动作 | 真实鼠标点击 → 上行 `{"op":"action","notice_id":"task-researchagent-086","action_id":"open","payload":"…"}` |
| 长稳 | 434s 内 0 次被动重连 |

### 14.2 仍未解决（已发 `tools/bridge-msg6.md`）

| # | 问题 | 数据 | 影响 |
|---|---|---|---|
| 1 | 每帧固定滞后 **68.5s**，恒定不增长 | 207 帧全量：t+0 到 t+434s 都是 68.4–68.5 | 用户看到的是 68 秒前的「正在执行」，实时提醒失效 |
| 2 | `hello` 无 `hello_ack` | 434s 内 0 条 | 本机身份为空；**按 terminal 定向无法验证** |
| 3 | 同 token 第二条连接收不到 `subscribed` 与任何帧 | 裸探针连 5 次、10–35s，全空 | 排障工具失效；「连上了但不推」与「没推」无法区分 |
| 4 | 无 `failed` 帧、`notice` 无 `target` | status: running 189 / success 17 / warning 1 / failed 0 | 失败色与重试按钮未在真链路验过；路由功能无法验收 |

### 14.3 这轮为此加的工具

- `tools/ws-sweep.py`：一次跑 7 种握手/订阅形态，每个变体独立计时，用来排除「字段名不对」。
- `tools/ws-header-sniff.py`：本地假 WS 服务，把客户端真实发出的握手头逐字打出来，
  用来确认差异在 header 还是在帧内容。
- `tools/wsprobe.py`：补上 op=0x2 / 0xA 的打印。原来只认文本帧，
  服务端若推二进制帧，表现是「连上了但什么都没收到」，最容易被误判。
- **客户端帧日志**：`ECHOCAST_WS_LOG=1` 启动，`%APPDATA%/com.echocast.client/ws-frames.log`
  逐帧记录收发。上面所有数字都出自它 —— 没有它，这四条问题一条都证明不了。

### 14.4 结论

客户端不需要为这四条改任何代码：拿到 `hello_ack` 就自动显示身份，
延迟修好后「1 分钟前」自动变「刚刚」，`target` 一旦出现就按终端路由。
现在能做的只有等，以及把证据留清楚。

排查工具：`python tools/wsprobe.py <url> <token>`（握手失败会把响应头原样打出来），
`python tools/ws-sweep.py`（多形态扫描），`python tools/ws-header-sniff.py 9797 30`（抓客户端握手）。
