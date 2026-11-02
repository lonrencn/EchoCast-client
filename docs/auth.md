# EchoCast 授权机制（两端对齐稿）

> 认证从哪产生、在哪验证、失败时两端各自怎么表现。事实来源：客户端代码
> （conn.rs / settings.rs / media.rs）、服务端交付的 terminal-routing.md 与
> notice-protocol.md、2026-11-02 会话对齐（署名契约 d94a708）。
> 标注 ⚠️ 的条目是两端口径尚未最终确认的点。

## 一图流

```
┌─ 产生（服务端）────────────────────────────────────────────┐
│ POST /network/join 一次性签发 token（明文仅返回一次）         │
│   · 现行真机族：net-…（48 位 hex）                           │
│   · 设计稿/演示族：ect_… + 48 hex（24 字节）        ⚠️ 关系待确认 │
│ 注册入口：甲=管理员建终端→手填；乙=8 位激活码(10 分钟)换 token   │
│ 服务端只存 sha256(token)，数据库备份泄漏 ≠ 终端被接管           │
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

## 上行入口与署名兑出（服务端 d94a708 契约）

| 入口 | 凭证 | 身份兑出 |
|---|---|---|
| WS 长连（桌面客户端） | net-token（本文件主链路） | terminal_id |
| `/network/publish`（api-token 直发） | api-token | `agent_name`：凭证自动兑出（S3-action / term_alpha…）或发布者显式传 |
| MCP 工具（agent 消息） | agent 凭证 | `agent_name`：同上，缺省退回 executor 名 |
| webhook 桥（应用端点上报） | 路由 ID 白名单 | `node_alias` = `webhook:<路由id>` |

客户端对 `agent_name` / `node_alias` **只呈现不验证**（属名小字，≤20 字符，
超长掩码）——它们是署名，不是权限。

## ⚠️ 待服务端确认（对齐未闭环项）

1. **net- 与 ect_ 前缀的关系**：`net-` 是现行真机族（2026-10-31 联调到手），
   `ect_` 出现在设计稿与本地演示（ws-demo.py）。是两族并存还是 ect_ 已废弃？
2. **token 生命周期**：是否设置过期？轮换流程（新旧并行窗口）？
   4029 说明同一 terminal 同时只允许一条连接——token 与终端是否严格一对一绑定？
3. **api-token 与 net-token 的签发/验证差异**：api-token（`api-…`）在哪一层
   验证、scope 模型是什么（参照 Gitea 的 read/write scope？）。
4. **HTTP 401 与 close 4001 的确切边界**：什么错误在握手层 HTTP 拒绝、什么
   在升级后 close 拒绝（客户端两种都停连，但要写进排障手册）。
5. **客户端 token 存储**：文档写「客户端存加密存储」，当前实现是
   settings.json 明文。是否需要落 Windows DPAPI（CredWrite）？

---
2026-11-02 · 客户端起草 · 服务端口径来自交付文档与 bridge 会话对齐记录
