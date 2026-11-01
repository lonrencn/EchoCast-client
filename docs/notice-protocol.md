# 通知推送契约

服务端把任务结果推给 EchoCast 客户端时，走的就是下面这一条 JSON。

客户端目前只在**本机进程内**用 Tauri 事件传递（事件名 `ec:notice`），
接上服务端后换成 WebSocket 文本帧即可 —— 载荷结构完全不变，
客户端从 `src/feed/notices.ts` 一个入口消费，不需要改组件。

---

## 1. 一条通知长什么样

`running`（执行中，带进度）：

```json
{
  "id": "task-8f3a-004",
  "task": "合成《传承的力量》第 4 段",
  "executor": {
    "name": "Soundao",
    "node": "Edge TTS",
    "kind": "local"
  },
  "status": "running",
  "result": "正在合成第 3–4 段",
  "progress": 0.62,
  "actions": [],
  "timestamp": 1790000000000,
  "channel": "soundao"
}
```

`success`（完成，带两个动作）：

```json
{
  "id": "task-8f3a-001",
  "task": "生成 Q3 市场趋势分析",
  "executor": {
    "name": "Research Agent",
    "kind": "agent"
  },
  "status": "success",
  "result": "报告已生成，共 26 页",
  "detail": "https://echocast.local/reports/q3-2026",
  "actions": [
    {
      "id": "open",
      "label": "查看结果",
      "style": "primary",
      "payload": "https://echocast.local/reports/q3-2026"
    },
    {
      "id": "copy",
      "label": "复制路径",
      "style": "ghost"
    }
  ],
  "timestamp": 1790000000000,
  "buddy_pack": "robot",
  "channel": "soundao"
}
```

以上两段是从 Rust 的 `serde_json::to_string_pretty` **实际输出**里拷出来的，
不是手写示意 —— 可选字段缺省时整个键会消失（见第 3 节）。

---

## 2. 字段表

| 字段 | 类型 | 必填 | 约束与用途 |
|---|---|---|---|
| `id` | string | **是** | 任务唯一 id。**进度更新必须复用它**，客户端靠它原地覆盖；换 id 就会多冒一个气泡 |
| `task` | string | **是** | 任务名，单行。客户端超过约 22 个字符就省略号截断，别把关键信息放在后面 |
| `executor` | object | **是** | 见下 |
| `executor.name` | string | **是** | 执行方名称，如 `Research Agent` |
| `executor.node` | string | 否 | 节点名，如 `GPU 节点 02`。有值时渲染成「名字 · 节点」 |
| `executor.kind` | enum | 否 | `local` / `remote` / `agent`，默认 `local`。决定图标 |
| `status` | enum | **是** | `running` / `success` / `warning` / `failed` |
| `result` | string | 强烈建议 | 结果摘要，两行封顶（≤ 60 字）。**只陈述事实和出路**，不要写「成功/失败」——颜色已经表达过了 |
| `detail` | string | 否 | 展开后的全文、日志尾部或结果 URL |
| `progress` | number | 否 | `0`–`1`，仅 `running` 有意义。越界会被夹到区间内 |
| `actions` | array | 否 | **最多 2 个**，超出的客户端截断 |
| `actions[].id` | string | **是** | 动作标识，点击时原样回传 |
| `actions[].label` | string | **是** | 按钮文案，由服务端给，客户端不硬编码「重试」 |
| `actions[].style` | enum | 否 | `primary` / `ghost`，默认 `ghost`。**同一条里 primary 最多一个** |
| `actions[].payload` | string | 否 | 点击后随动作回传的载荷（URL、命令名等） |
| `sticky` | bool | 否 | 不传则按 status 推断：`failed` / `warning` 为 true |
| `timestamp` | int64 | **是** | **毫秒**级 Unix 时间戳。客户端相对时间（「刚刚」「2 分钟前」）依赖它 |
| `buddy_pack` | string | 否 | 指定用哪套形象包；不传跟随用户全局设置 |
| `channel` | string | 否 | 分组，默认空串。对应频道概念 |

### 枚举取值

```
status   : running | success | warning | failed
kind     : local | remote | agent
style    : primary | ghost
```

全部小写下划线风格（snake_case）。

---

## 2.1 进度条怎么给

`progress` 是**唯一**驱动进度条的字段，就一个 `0`–`1` 的浮点数，没有别的形态：

```json
{ "status": "running", "progress": 0.62 }
```

几条约定，都是踩过的坑：

| 情况 | 怎么做 | 为什么 |
|---|---|---|
| 百分比 | `0.62`，不是 `62` | 传 `62` 会被夹到 `1.0`，进度条直接满格不动 |
| 刚起步 | `0.02` 起，别给 `0` | 0 和「没有进度」在视觉上分不清，用户以为卡住了 |
| 不知道进度 | **整个字段省略** | 不传就不渲染进度条，只显示 `result` 文案。编一个假的百分比比没有更糟 |
| 无法量化（流式输出、排队中） | 省略，用 `result` 写「排队第 2 位」 | 进度条只能表达比例，表达不了位置 |
| 到 100% | 直接推终态 `success`，不要推 `progress: 1` | 满格进度条停在那是死画面，终态才有收尾动作 |

两个客户端行为，服务端不用重复实现：

- **同 id 原地更新**：从 0 到 100 推 20 次都用一个 `id`，气泡不会刷屏。
- **2% 阈值**：变化小于 2% 不重绘，避免百分比数字疯狂跳动。所以推太密没意义，**1–3 秒一次**足够。

`result` 在 `running` 时是「进度说明」，服务端每推一次进度就顺手换一句
（「合成第 3–4 段」→「混流与响度归一」），这比百分比数字更有信息量。

---

## 3. 可选字段请「省略」，不要给 null

Rust 侧用了 `skip_serializing_if = "Option::is_none"`，所以缺省的字段**整个键不存在**：

```json
// 正确：没有 node 就不写这个键
{ "name": "Research Agent", "kind": "agent" }

// 不建议：给 null
{ "name": "Research Agent", "node": null, "kind": "agent" }
```

同理 `progress`、`detail`、`sticky`、`buddy_pack` 缺省时都直接不出现。
`actions` 是例外，执行中请给空数组 `[]` 而不是省略。

---

## 4. 状态机（服务端必须遵守）

```
running ──┬─> success
          ├─> warning
          └─> failed
```

1. **终态不可逆**：一旦推了 `success` / `warning` / `failed`，就不要再推同 id 的 `running`。
   客户端有防护（终态不会被进度覆盖），但服务端不该依赖这个兜底。
2. **进度更新复用 id**：一个任务从 0% 到 100% 推 N 次，每次都是同一个 `id`，
   客户端只更新那一条气泡，不会刷屏。
3. **执行中不要下发 actions**：`running` 时给「重试」按钮，用户连点会重复触发任务。
   客户端也会拦（`running` 通知不带动作），但同样不该依赖兜底。
4. **终态后不要复用 id**：同一个 id 再次出现在 `running` 状态，会被当成「这条任务又跑起来了」。
   一个任务真的要重试，请生成新的 id。

---

## 5. 客户端会自动做的事（服务端不要重复实现）

| 行为 | 规则 |
|---|---|
| 停留时长 | `success` 5 秒后自动收起；`warning` 12 秒；`failed` **常驻不消失** |
| 去重 | 同 `id` 覆盖，不新增 |
| 同屏上限 | 最多 3 条气泡，其余折叠成「还有 N 条」 |
| 相对时间 | 客户端按 `timestamp` 自己算，不需要服务端格式化 |
| 动作数量 | 超过 2 个截断 |
| 进度条 | 变化小于 2% 不重绘（避免步进式跳动） |

也就是说：**服务端只负责推事实，不要推「这条要显示几秒」这类表现参数。**

---

## 6. 动作回传

用户点击气泡上的按钮后，客户端会**在同一条 WS 连接上**发一帧上行消息：

```json
{
  "op": "action",
  "notice_id": "task-8f3a-001",
  "action_id": "open",
  "payload": "https://echocast.local/reports/q3-2026"
}
```

| 字段 | 来源 |
|---|---|
| `notice_id` | 通知里的 `id`，原样回传 |
| `action_id` | 通知里的 `actions[].id`，原样回传 |
| `payload` | 通知里的 `actions[].payload`；没给就整个键不出现 |

**这条链路依赖服务端提供 `id`。** 通知里没有 `id` 时客户端会用
「任务名 + 执行方」算一个指纹（`auto-3f9c…`）顶上，但那只是本地标识，
服务端查不到对应任务，动作路由会断。

连接没建立时点动作不会假装成功：气泡留在原地，用户可以再点一次。

### 6.0 已读回执：`ack` + `state:"seen"`

除动作之外，客户端还会自动上报「这条我看到了」：

```json
{ "op": "ack", "task_id": "task-videoshotcraft-148", "state": "seen" }
```

| 时机 | 发不发 |
|---|---|
| 收到任意通知（广播 `op:"notice"` 或定向 `task.deliver`） | 发，**同一 `id` 只发一次** |
| 同一条任务的后续进度帧 | 不发（看过了就是看过了，不是看了几眼） |
| 断线重连后收到的补投 | 发（这台终端之前确实没看到） |

`state` 另外三个值 `accepted` / `rejected` / `done` 由用户级操作触发，
目前只在服务端把动作 id 定为 `accept` / `reject` 时才自动跟发 ——
**想收到任务级状态，就得把按钮 id 定成这两个词**，或者在 `ask.options[].id`
里用它们。

> 实测（2026-10-31）：`ack` 帧发出后服务端**没有任何回应**，既没有确认帧也没有报错。
> 如果服务端确实在处理，建议回一帧 `{"op":"ack_ok","task_id":…}`，
> 否则客户端无法区分「被接受」和「被忽略」。

### 6.1 问询：需要人给一个答复

`actions` 是「你可以做点什么」，点完就结束；**`ask` 是「我在等你选一个」**。
凡是任务卡在人的判断上（要不要继续、发不发、选哪一版），都应该用 `ask`
而不是塞一个 `重试` 按钮 —— 客户端会把它显示成「等你答复…」，并在没人
理会时**反复出声**（这套提示音是整个客户端里唯一会重复的）。

```json
{
  "id": "task-8f3a-007",
  "task": "方言保留微调 · 第 3 轮",
  "status": "warning",
  "result": "显存峰值 23.8 / 24 GB，已经贴顶",
  "ask": {
    "question": "降 batch 重跑，还是今晚先停？",
    "options": [
      { "id": "retry-low", "label": "降到 batch 4 重跑", "style": "primary" },
      { "id": "hold", "label": "今晚先停" }
    ],
    "deadline": 1790000000000,
    "context": "ctx-7f3a"
  }
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `ask.question` | 是 | 一句话问题。气泡放不下，只在展开面板里显示 |
| `ask.options[]` | 是 | **至少 2 个**、最多 4 个。少于 2 个客户端当普通通知处理 |
| `ask.options[].id` | 建议 | 答复时原样回传。不给的话客户端只能用下标 `opt-0`，服务端对不上 |
| `ask.options[].style` | 否 | `primary` 只标一个；标多了等于没标 |
| `ask.deadline` | 否 | 毫秒时间戳。过期只是提示，仍然可答 |
| `ask.context` | 否 | 原样回传的上下文，客户端不解释 |

**答复帧跟动作帧是同一种，不用新开一种帧型：**

```json
{ "op": "action", "notice_id": "task-8f3a-007", "action_id": "retry-low", "payload": "ctx-7f3a" }
```

`action_id` 就是选项的 `id`，`payload` 取 `options[].payload`，没有就用 `ask.context`。

**几条客户端会做、服务端不必重复的事：**

- 问询一律**常驻**，不会自己消失；同时给了 `actions` 也以 `ask` 为准（动作区被让位）。
- 到达时用第三套提示音（同音三连，dididi），之后每 18 秒再响一次，
  **最多三次**；用户答复后立即停。
- 答复完气泡显示「已答复 · <选项文案>」，6 秒后收起。
  **服务端如果不推新状态，这条任务在客户端就到此为止了** —— 想让它继续，
  用同一个 `id` 推一帧新状态即可。
- 静音开关对问询一样生效；关掉就是彻底没有声音，界面上的呼吸和「等你答复…」还在。

> 兼容：也可以把 `question` 与 `options` 平铺在通知顶层，客户端认。
> 但别两个位置都给，以 `ask` 对象为准。

### 6.2 回执：服务端必须告诉客户端「收下了没有」

`action` 帧是**单向**的。客户端把帧写进 WS 发送队列就返回成功了，
这**不代表服务端收到了**。没有回执，界面就只能给一个假的「已答复」——
用户以为改掉了，实际服务端什么都没发生。这是比报错危险得多的事。

所以服务端收到动作/答复后，必须回一帧：

```json
{ "op": "action_ack",  "notice_id": "task-8f3a-007", "action_id": "retry-low" }
```

```json
{ "op": "action_nack", "notice_id": "task-8f3a-007", "action_id": "retry-low",
  "reason": "already_answered" }
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `op` | 是 | `action_ack` / `action_nack`。客户端按前缀认（`action.*`、`action_result` 都行），**但不要用裸 `action`** —— 那是上行的名字，会被当成回声跳过 |
| `notice_id` | 是 | 原样带回下发时的 `id`。空的话整帧被丢弃（对不上是哪条，改不了界面） |
| `action_id` | 建议 | 带回用户点的那个。与本地记录不一致时当迟到回执处理 |
| `reason` | nack 必填 | 会显示给用户，请写人能懂的词。已内置翻译：`unknown_notice` / `already_answered` / `expired` / `forbidden`；其它原样显示 |

`ok` / `success` / `accepted` 布尔字段也认，用在没有 `op` 的裸回执上。

**客户端收到之后：**

| 收到 | 行为 |
|---|---|
| `ack` | 维持原样，气泡按 6 秒收起 |
| `nack` | 撤掉收起定时器，气泡留在原地显示原因，**选项重新出现**允许改选 |
| 4.5 秒内无任何回执 | 标「已发出 · 待确认」（琥珀色，不是绿色），并延长到 20 秒才收起 |

「没回执」不等于「失败」——慢和丢是两回事，混在一起会误报。但也不能算成功。

### 6.3 媒体附件：`media`

通知可以携带多媒体文件，客户端负责下载（带缓存）与展示：

```json
{
  "id": "task-8f3a-010",
  "task": "生成营销主视觉",
  "status": "success",
  "result": "已产出 2 张图 + 1 段音频",
  "media": [
    { "kind": "image", "url": "https://…/poster.png", "name": "主视觉.png", "size": 204800 },
    { "kind": "audio", "url": "https://…/voice.mp3",  "name": "配音.mp3" },
    { "kind": "video", "url": "https://…/demo.mp4",   "poster": "https://…/thumb.jpg" },
    { "kind": "file",  "url": "https://…/data.csv",   "name": "数据.csv" }
  ],
  "timestamp": 1790000000000
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `media[].kind` | 建议 | `image` / `audio` / `video` / `file`。缺省时客户端按 `mime` → URL 扩展名猜 |
| `media[].url` | **是** | **必须是 http(s) 可 GET 拉取的资源**。base64 内嵌、`file://` 一律丢弃该条 |
| `media[].name` | 建议 | 显示名。缺省取 URL 最后一段（带百分号解码） |
| `media[].mime` | 否 | 帮助判断类型（`image/png`、`audio/mpeg`…） |
| `media[].size` | 否 | 字节数，仅用于展示。给浮点也认（`2048.0`） |
| `media[].poster` | 否 | 视频封面图 URL |

客户端这边的约定：

- 字段名兼容 `media` / `attachments` / `files`；条目内的 `url`/`src`/`href`、
  `name`/`filename`/`title` 都认。**但别两个位置都给，以 `media` 为准。**
- 单条通知最多展示 12 个附件，超出的丢弃。
- 下载在 Rust 侧完成（不走 WebView，不受 CORS 约束），缓存到
  `%APPDATA%\com.echocast.client\media-cache\`，**同 URL 只下载一次**。
  鉴权：拉取时带 `Authorization: Bearer <token>`（与 WS 连接同一个 token）；
  URL 自带短时签名同样可行 —— 两种都支持，服务端任选。
- 单文件下载上限 200MB，超限报错；自签证书与 WS 同策略（接受）。
- 附件名允许的写法：URL 带 query 没关系（`?sig=…&exp=…` 签名 URL 完全可行，
  签名变了会当作新文件重新下载，行为正好符合预期）。
- `running` 状态的通知也会展示媒体区（执行中先给封面是合理诉求）。
- 气泡上出现「📎 N 个文件」角标；点开详情面板看到媒体区：
  图片网格（点击放大）、音频/视频内嵌播放器、文件卡片（打开按钮，
  用系统默认程序，**只允许打开媒体缓存目录内的文件**）。

联调可以用自带的假服务端直接验：

```bash
python tools/ws-demo.py --plain     # 每 7 秒一条任务，终态带 3 个媒体附件
# 媒体文件在 tools/media-sample/，由 http://127.0.0.1:8787/files/<name> 提供
```

---

## 7. 走 WebSocket 时的帧格式

**客户端已经接好了**：设置面板里填地址，Rust 侧就会连上去收帧，
两种格式都认，服务端选一种即可。

```jsonc
// 甲：每帧就是一个通知对象（推荐，最省事）
{ "id": "task-8f3a-004", "task": "...", "status": "running", "progress": 0.62, ... }

// 乙：包一层信封，方便以后混入心跳、订阅确认
{ "op": "notice", "notice": { "id": "task-8f3a-004", ... } }
```

解析逻辑在 `src-tauri/src/conn.rs` 的 `parse_notice()`：
先按裸通知试，失败再按信封试。**两种都不认的帧会被静默丢弃**
（心跳、订阅确认这类回包不会报错），所以控制帧可以放心发，但别把通知包在更深的层级里。

其他连接层行为：

| 行为 | 客户端怎么做 |
|---|---|
| 鉴权 | **握手时带 `Authorization: Bearer <token>`**（设置面板里填）。token 是入网 `POST /network/join` 时一次性签发的 48 位 hex，仅此一次返回 |
| 订阅 | 连上后立刻发 `{"op":"subscribe","topics":["…"]}`。服务端**不做通配**，必须逐个列出 |
| 入口 | 自行部署后形如 `wss://<你的服务端域名>:8443/ws`；内网形如 `ws://<内网IP>:8791/ws` |
| 心跳 | 每 25 秒发一次 WS 协议层 `Ping`，服务端按标准回 `Pong` 即可，不用自定义业务心跳 |
| 断线重连 | 无限指数退避：1s → 2s → 4s …… 上限 30s。服务端晚几分钟起来也能自己连上 |
| 主动断开 | 设置面板点「断开」后不再重连 |

### 要订阅哪些 topic

服务端精确匹配、不做通配，客户端默认订阅下面这一组（`DEFAULT_TOPICS`）：

| 域 | 事件 |
|---|---|
| `task.*` | `task.started` / `task.running` / `task.completed` / `task.failed` / `task.warning` |
| `alert.*` | `alert.critical` / `alert.warn` |
| `deploy.*` | `deploy.done` |
| `flimx.*` | `flimx.project.updated` / `flimx.asset.missing` / `flimx.subtitle.generated` |

新增域时**双方都要改**：服务端加 topic，客户端把它加进默认值（或让用户在设置面板里手填）。

### 控制帧

服务端可能发过来的非通知帧，客户端会**静默丢弃**，不会变成气泡：

```json
{ "op": "subscribed", "topics": ["task.completed"] }
{ "op": "unsubscribed", "topics": [] }
{ "op": "pong" }
```

注意别把通知包在更深的层级里（`{op:"notice", notice:{...}}` 只剥这一层）。

---

## 7.1 服务端地址配在哪

不写死在代码里，也不写在前端 localStorage —— 连接由 Rust 发起，它读不到 WebView 的存储。

| 项 | 位置 |
|---|---|
| 配置入口 | 窗口右上角**齿轮**，或点状态灯 |
| 存盘位置 | `%APPDATA%\com.echocast.client\settings.json`（设置面板底部直接显示完整路径，可点复制） |
| 字段 | `server_url` / `auto_connect` / `mock_enabled` / `buddy_pack` / `protocol` / `token` / `topics` |

地址允许三种写法，客户端自动规范化：

```
ws://127.0.0.1:8766/ws   → 原样使用
http://127.0.0.1:8766/ws → ws://127.0.0.1:8766/ws    （端口相同，顺手转）
127.0.0.1:8766/ws        → ws://127.0.0.1:8766/ws    （裸地址补协议）
```

`wss://` 走 TLS，已支持（Windows 走系统 SChannel，不需要额外装证书库）。

**配了地址就不再跑本机 mock** —— 两路数据同时往浮层推，排障时分不清哪条是真的。
想回头再 mock：设置里关掉地址（清空）或把「启动时自动连接」关掉后重启。

---

## 7.2 字段不全时客户端怎么兜底

联调期服务端常常先推 `{topic, text, card}` 这种偏展示的结构。客户端
（`src-tauri/src/wire.rs`）会把它归一化成完整通知，所以**字段缺失不会白屏**：

| 缺什么 | 客户端怎么补 |
|---|---|
| `id` | 用「任务名 + 执行方」算 FNV 指纹，前缀 `auto-`。同样的内容算同一个 id，重复推进度会原地覆盖 |
| `status` | 先看 `status`，再看 `topic` 关键词（`completed`→成功、`fail`→失败…），再看有没有 `progress`，都没有就当 `success` |
| `progress` | 没有任何兜底。传 `62` 这种百分比会自动除以 100 |
| `timestamp` | 取收到帧的时刻。10 位秒级时间戳自动乘 1000 |
| `task` / `result` | 从 `card.fields` 的 `任务`/`结果` 取；再不行从 `text` 的 `任务 - 结果` 拆 |
| `executor.name` | 用 `card.source` |
| `actions` | 把单数的 `card.action` 提升成只有一个元素的数组，`id` 默认 `open` |

**这层是兼容，不是契约。** 两件事它救不了：

1. **动作回传**：指纹只在本机唯一，服务端认不出 `auto-…`，路由会断。
2. **同名任务撞车**：两个同名同执行方的任务会共用一个指纹。

所以 `id` 该由服务端给。认不出的帧（心跳、订阅确认）一律丢弃，
不会凭空造出一条空气泡。

---

## 7.3 终端路由（定向投递）

前面几节解决的是「怎么显示一条通知」。如果服务端要让**不同任务发给不同机器**，
还需要一层终端身份 —— 完整方案见 **[terminal-routing.md](terminal-routing.md)**。

一句话概括：**token 决定你是谁，target 决定发不发给你。**

| 概念 | 作用 | 谁决定 |
|---|---|---|
| `topic` | 事件类型（what） | 客户端订阅，只是过滤器 |
| `target` / `terminal_id` | 投递对象（who） | **服务端**按 token 反查，客户端无权干涉 |
| `ack` | 任务到了哪个阶段 | 客户端回 seen，用户回 accepted / rejected / done |

客户端这边已经接好，服务端只需要做到：

1. 握手时用 `Authorization: Bearer <token>` 查出 `terminal_id`，把这条连接绑定到它；
2. 建任务时解析 target，只推给命中的连接；
3. 认 `{"op":"ack"}` 上行帧。

在它落地之前，现有广播式通知（第 1–7 节）完全不受影响 —— 两者可以并存。

---

## 8. 怎么自己验

### 用自带的假服务端（推荐）

仓库里有一个零依赖的最小 WS 推送服务，它推的字段就是本文档第 1 节的示例：

```bash
python tools/ws-demo.py          # 监听 ws://127.0.0.1:8787，Ctrl+C 退出
python tools/ws-demo.py 9000     # 换端口
```

然后在客户端**齿轮 → 地址填 `127.0.0.1:8787` → 连接**。它会每 7 秒推一条完整任务
（5 次进度更新 + 1 次终态），可以拿来对照每个字段的渲染效果。

写真实服务端时，照抄它的推送节奏就行 —— 那份代码就是「最小可行服务端」。

### 用内置 mock

没配服务端地址时，桌面端会自动跑内置 mock：

```bash
pnpm tauri dev
```

窗口右上角状态灯显示 `Rust IPC · ec:notice` 说明走的是真实事件链路。
**配了地址之后 mock 会自动停用**，避免真假数据混在一起。

### 对照字段

跑契约测试，它会打印真实的序列化结果：

```bash
cd src-tauri && cargo test --lib print_wire_contract -- --nocapture
```

---

## 9. 一个常见的坑

`timestamp` 是**毫秒**。如果服务端给的是秒（10 位），客户端会把时间算到 1970 年，
气泡上会显示成 `01-01 08:00` 这种明显不对的时间。验的时候看一眼时间戳位数：
13 位才对。
