# EchoCast 授权机制（两端对齐稿）

> 认证从哪产生、在哪验证、失败时两端各自怎么表现。事实来源：客户端代码
> （conn.rs / settings.rs / media.rs）、服务端交付的 terminal-routing.md 与
> notice-protocol.md、2026-11-02 会话对齐（署名契约 d94a708）。
> 标注 ⚠️ 的条目是两端口径尚未最终确认的点。

## 一图流

```
┌─ 产生（服务端）────────────────────────────────────────────┐
│ 唯一签发入口：POST /network/join（邀请码恒时校验，fail-closed）  │
│   · 激活码入口换发的 token 为 net-12 位短码（11-03 实测 16 字节总长）│
│   · 明文不落库——join/activate 响应后服务端只存 sha256（network.json）│
│   · 签发即绑定：join 时生成 agent 记录（agent_id/名字/能力/hash）│
│   · 无过期；轮换=重新 join；吊销=删 agent 记录                  │
│   · 多终端并发使用：允许（实测）                                │
└──────────────────────────────────────────────────────┘
              │ 用户粘进设置面板（settings.json）
              ▼
┌─ 携带（客户端）────────────────────────────────────────────┐
│ WS Upgrade 头 Authorization: Bearer <token>                  │
│   · 必须握手时带（服务端在 connection 事件比对 hash，首帧补不了）│
│   · 空 token = 匿名终端，只收 broadcast（fail-closed）        │
│ media 附件拉取复用同一 token（Authorization: Bearer）          │
└──────────────────────────────────────────────────────┘
              ▼
┌─ 验证（服务端）────────────────────────────────────────────┐
│ sha256(token) 查 terminals 表                                │
│   · 查不到 → HTTP 401 / close 4001                           │
│   · revoked_at 非空 → 403 / close 4003                       │
│   · 同 terminal_id 已有连接 → close 4029（单实例）            │
│ 通过 → 绑定 连接↔terminal_id，status=online                   │
│ hello 帧：alias/labels/版本 只是建议值，管理员改过的覆盖回来    │
│ hello_ack：terminal_id 只能由服务端下发——客户端自报 id 一文不值 │
└──────────────────────────────────────────────────────┘
              ▼
┌─ 消费（两端共同的规则）────────────────────────────────────┐
│ token 决定你是谁，target 决定发不发给你                        │
│ subscribe 的 topics 只是事件类型过滤器，不参与认证与路由        │
└──────────────────────────────────────────────────────┘
```

## 客户端失败语义（conn.rs，测试覆盖 `auth_close_codes_stop_retrying`）

| 服务端表现 | 客户端行为 |
|---|---|
| HTTP 401 / 403（握手层） | **停止重连**，提示改配置——配置层错误重连一万次也不会好，还会刷爆服务端日志 |
| close 4001（token 无效/过期） | 停止重连，提示重新填写 |
| close 4003（已被吊销） | 停止重连，提示联系服务端 |
| close 4029（别的实例占用） | 停止重连并提示，避免两台机器互踢 |
| close 1000 / 1005 / 其他 | 指数退避无限重连（1s → 30s 封顶） |

## 身份的三层来源

| 层 | 产生 | 验证 | 谁说了算 |
|---|---|---|---|
| **凭证（token）** | 服务端 `/network/join` 签发 | 服务端握手时 hash 比对 | 服务端 |
| **终端身份（terminal_id）** | 服务端按 token 反查兑出，hello_ack 下发 | 连接绑定表 | 服务端（客户端自报无效） |
| **设备指纹（device_id）** | 客户端本地生成，落 `%APPDATA%/device.json` | 不验证，仅 hello 帧上报辅助排障 | 客户端 |

> 路由的关键约束：**身份只能来自凭证，不能来自报文**。任何客户端可填的身份
> 字段都等于没有权限——服务端按 `token → terminal_id` 绑定连接，再按
> `target` 决定推给谁。

## 上行入口与署名兑出（服务端 d94a708 契约 + 11-03 确认）

| 入口 | 凭证（产生） | 验证 | 身份兑出 |
|---|---|---|---|
| WS 长连（桌面客户端） | net-token（/network/join 签发） | orchestrator WS connection 事件查 hash | terminal_id |
| `/network/publish`（net-token 直发） | 同上 | 同一 hash 表 | `agent_name` = 入网注册名 |
| `/network/publish`（api-token） | 管理台「API 令牌」页签发（明文落 api_tokens.json，可绑 session_id） | 同一验证点 | `agent_name` = 令牌备注名（S3-action…） |
| MCP 工具 echocast_notify | 管理 bearer（.env） | 同上 | `agent_name` = executor 或 body 覆写 |
| webhook 桥（应用端点上报） | 路由 secret（token / HMAC） | 路由白名单 | `node_alias` = `webhook:<路由id>` |

客户端对 `agent_name` / `node_alias` **只呈现不验证**（属名小字，≤20 字符，
超长掩码）——它们是署名，不是权限。body 里显式 `agent_name` 可覆盖为显示别名。

## 令牌申请与发放（2026-11-03 对齐中）

**现状（服务端自述）**：文档里的两种入口都**未落地**——实际流程是「找管理员要
静态邀请码 → `/network/join` → 手抄 48 位 hex 填进设置」。邀请码是永不过期的
共享密钥；没有审批、没有激活码、没有找回。这是「用户不知道在哪申请令牌」的根因。

**正在落地（激活码流程，terminal-routing.md §4.1 乙）**：

```
管理员生成 8 位一次性激活码（10 分钟有效）
        ↓ 任何途径发给用户（IM / 邮件）
用户在客户端设置面板 →「没有令牌？用激活码入网」
        ↓ 输码 +（可选）这台机器的别名
客户端 POST {server}/api/terminals/activate
        body {code, alias, device_fp, os, app_version}
        ↓ 200
{terminal:{id, alias, labels}, token}   ← token 只此一次返回
        ↓ 客户端自动落设置并连接
hello → hello_ack(term-…) → 气泡弹出来，全程不碰 hex
```

- 409 = 码已被用（一码一机）；410 = 码过期；404 = 服务端尚未升级出激活接口
- 换机器/丢失：重新申请码入网；旧 terminal 由管理员在 agent 记录里删除（即吊销）
- 多终端并发同一 token：允许（见上文生命周期）

**客户端配套（v0.2.6）**：`terminal_activate` 命令 + 设置面板激活码入口 +
错误文案带指引。REST 基址自动从 WS 地址推导（wss→https），用户只填一次服务端地址。

## ⚠️ 待服务端确认（对齐未闭环项）

> 2026-11-03 已闭环：服务端逐条回复（他们侧文档 commit `049be18`，与本文配对），
> 原五问的答案已合并进上文各节。保留此节作为变更记录。

| 原问题 | 确认结果（2026-11-03） |
|---|---|
| net- / ect_ 前缀关系 | **唯一现行族是 `net-[0-9a-f]{48}`**（NETWORK_TOKEN_RE 单源）；`ect_` 是早期设计稿/演示写法，不在真实链路上 |
| token 生命周期 | **无过期**，长期有效；轮换 = 重新 join（旧 token 走吊销）；吊销 = 管理端删 agent 记录（hash 消失即失效）；**多终端并发允许**（实测） |
| api-token 验证 | 管理台「API 令牌」页签发（明文落 api_tokens.json，可绑 session_id）；`agent_name` 兑出为令牌备注名 |
| HTTP 401 vs close 4001 | **WS 路径统一 4001**（upgrade 后 connection 事件里关）；**HTTP REST 统一 401**。客户端两条停止重连策略正好对应，无需改 |
| 客户端明文存储 | 待办不变（是否落 DPAPI 由客户端排期） |

⚠️ 两个已知偏差（服务端路线图）：
1. **4003 当前不发**——吊销后的 token 表现为 4001（与填错同码）。客户端
   「4001 停止重连」策略效果等价；要区分「吊销 vs 填错」需服务端加吊销标记。
2. 验证点**唯一**在 orchestrator 的 WS connection 事件——bridge（TCP 透传）
   与 NPM（代理）均无鉴权逻辑。排障时别在这两层找认证问题。

---
2026-11-02 · 客户端起草 · 服务端口径来自交付文档与 bridge 会话对齐记录
