# 授权机制 v2 提案：自生成身份 · 双向登记 · 可轮换 · 多对多

> 状态：**提案（待服务端确认）** · 现行 v1 模型见 [auth.md](auth.md)，两者可并行
> 起因：v1 的「服务端签发 → 用户线下配送」把「秘密配送」变成了所有摩擦的根源
> （激活码/审批/入网指引/手抄 hex，全是在把一个秘密从 A 机安全送到 B 机）。
> v2 把秘密留在生成它的机器上，登记只交换**标识符**。

## 1. 角色

| 术语 | 产生方 | 说明 |
|---|---|---|
| `device_token` | **桌面端本地自生成**（32 hex 短 token；进阶可换 Ed25519 密钥对） | 桌面对服务端的唯一标识符，秘密永不出本机 |
| `server_id` | **服务端自生成**（`srv-` + hex），登记响应下发 | 服务端对桌面的标识符，桌面 pin 住它防串服务器 |
| `terminal_id` | 服务端在登记时分配（`term-…`，与 v1 同形） | 一次登记一条，一标多投时一对多 |
| 登记关系 | (server_id, sha256(device_token)) → terminal_id | 服务端 `network.json` 的一条记录，v1 的存储模型不变 |

## 2. 时序

### 2.1 首次登记（join-v2）

```
客户端                                        服务端
① 本地生成 device_token（不外发）
② POST /network/register-v2
   { device_token, invite?, alias, device:{…} } ──►  门槛校验（见 §4）
                                                    存 sha256(device_token)
                                                    生成 server_id（若无）
◄── { terminal:{id,alias,labels}, server_id, protocol:"2" }
③ 客户端持久化绑定：(endpoint_url, device_token, server_id)
```

### 2.2 日常连接

> **实现状态（2026-11-04）**：服务端 register-v2 / rotate-v2 已实现
> （响应形状见下），**待网关暴露**——bridge 网关当前对 `/network/*` 返回
> unknown route（v1 的 join 同样未挂公网，这正是 v1 只能线下签发的原因）。
> 已提请挂路由（bridge-msg41），客户端 v2 代码已就绪，路由通了即联调。

```
Authorization: Bearer <device_token>          ← 与 v1 同形，hash 查表
X-Server-Id: <server_id>                      ← 新增：客户端校验服务端身份
服务端：hash 命中 → 绑定连接
        server_id 不匹配 → close 4004（令牌属于另一台服务器）
客户端：首帧比对 server_id，不一致 → 断开并提示「令牌指向别的服务器」
```

双向校验各防一类错：服务端防冒充（hash 表），客户端防误指/钓鱼（pin）。

**register-v2 实测响应形状**（服务端 2026-11-04 交付）：

```json
{
  "terminal_id": "net-eb42c3d7d56a06e1b2a9acd8",   // net- 前缀 + 长 hex（服务端编号规则）
  "server_id": "bf582afd198ac729",                  // = sha256(ORCHESTRATOR_BEARER) 前 16 位
  "token": "net-DTAX8B76CTF5"                       // ← WS 连接凭证（12 位短码）
}
```

校验规则（服务端原话）：device_token 须 32 位 hex（客户端本地生成），
服务端存 sha256（device_token_hash），明文不落库。rotate-v2 换 hash 后旧连接
立即失效。close 4004（server_id 不匹配）服务端**下轮实装**；客户端已先行
发送 X-Server-Id 头并实现 4004 语义（fatal 提示 + 停止重连）。

### 2.3 轮换（rotate-v2）

```
客户端本地重新生成 device_token'
POST /network/rotate-v2 { old: device_token, new: device_token' }
服务端：原子换 hash（旧连接失效，新连接即生效）→ terminal_id / server_id 不变
```

### 2.4 销毁

- 服务端删 terminal 记录（= v1 吊销，后续连接拒）
- 桌面端删除本地绑定（= 从该服务端注销）

### 2.5 一标多投

同一 `device_token` 分别向 N 个服务端登记 → N 条独立绑定
（各服务端各发各的 server_id/terminal_id）。客户端「多端点」列表
每条端点存 `(url, device_token, pinned server_id)`，天然多对多。

## 3. 门槛（登记授权，替代 v1 的「签发审批」）

v2 里门槛只承载**授权决定**，不承载秘密——这是与 v1 的本质区别：
邀请码被截获 = 别人获得了「申请资格」，不是获得了凭证。

| 门槛 | 实现 | 适用 |
|---|---|---|
| A 一次性邀请码 | 登记请求带 `invite`，服务端核销 | 现有激活码流程平滑迁移（见 §5） |
| B 待审队列 | 登记入 pending，管理员批准后激活 | 开放自助申请 |
| C 内网限定 | 仅监听内网/VPN 地址 | 完全可信环境 |

三者可并存（如：内网直登 + 外网走邀请码）。

## 4. 已知权衡（不回避）

1. **TOFU 假设**：首次登记时的服务端必须是真的（与 SSH known_hosts 同级）；
   server_id pin 只防「之后的」误指与钓鱼，不防「第一次」。
2. **门槛即边界**：门槛 A 泄漏 = 别人可登记（但泄漏的是资格不是凭证，
   且一码一机 + 时效可控；比 v1 的明文令牌配送轻一个量级）。
3. 非对称升级路径：`device_token` 未来可替换为 Ed25519 公钥登记
   （服务端永远只存公钥，抗服务端库泄漏更强），协议字段不变。

## 5. 与 v1 的兼容

| 项 | v1（现行） | v2（本提案） |
|---|---|---|
| 凭证产生 | 服务端 join 签发 | 客户端本地生成 |
| 激活码 | 签发 net-token | 变成门槛 A：invite 字段伴随登记 |
| server_id | 无 | 新增，双向校验 |
| 4001/4003/4029 | 停止重连 | 语义不变，新增 4004（跨服务器） |
| 多端点 | 各端点各存 token | 各端点各存 device_token + pin |

服务端可**双凭证并行**（network.json 同时收 net-token 与 v2 登记），
客户端按 `/network/register-v2` 是否存在探测降级到 v1。

## 6. 工作量

- 服务端：register/rotate 两端点 + server_id 生成与校验 + 4004（小；
  join/hash 存储模型完全复用）
- 客户端：本地生成与绑定存储 + pin 校验 + 多端点绑定字段（小；
  激活码 UI 平滑迁移为「登记门槛」入口）
