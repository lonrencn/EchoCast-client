# 形象包规范

气泡里的小形象不是写在组件里的，是一个可以整套替换的**素材包**。
换包只改一个 id，代码零改动。

## 目录

```
src/buddies/
  builtin/              # 内置包，构建期自动发现
    <pack-id>/
      manifest.json
      idle.webp
      success.webp
      fail.webp
      running.webp      # 可选
  types.ts              # 契约
  packs.ts              # 发现 / 校验 / 回退 / URL 解析
  remote.ts             # 用户包加载（Tauri）
  useBuddy.ts           # 组件唯一入口
```

**新增一个内置包 = 建一个文件夹。** 不需要改 `packs.ts`，也不需要任何登记：
`import.meta.glob` 在构建时把它收进来。

用户导入的包放在 `$APPDATA/buddies/<pack-id>/`，由 Rust 的 `buddy_user_packs` 扫描，
图片走 Tauri asset 协议（scope 已限定为 `$APPDATA/buddies/**`）。

## manifest.json

```json
{
  "pack": "robot",
  "version": 1,
  "frame": 256,
  "display": 96,
  "baseline": 245,
  "primary": true,
  "actions": {
    "idle":    { "file": "idle.webp",    "frames": 1, "duration": 2400, "loop": true },
    "success": { "file": "success.webp", "frames": 1, "duration": 560,  "loop": false, "hold": "last" },
    "fail":    { "file": "fail.webp",    "frames": 1, "duration": 500,  "loop": false, "hold": "last" },
    "running": { "file": "running.webp", "frames": 1, "duration": 1600, "loop": true }
  }
}
```

| 字段 | 说明 |
|---|---|
| `frame` | 出图边长。256 是当前标准，显示 40–150px 都不糊 |
| `display` | 建议显示尺寸，仅作参考，实际由组件决定 |
| `baseline` | 脚底基线 y。**所有帧必须共用同一条**，否则播放时角色上下跳 |
| `primary` | 主推形象，排在最前并作为默认 |
| `frames` | 雪碧图帧数。1 表示单帧静态 |
| `loop` | 循环动作（idle/running）为 true，落定动作（success/fail）为 false |
| `hold` | 非循环动作停在哪一帧 |

## 动作与回退

必填三项：`idle`、`success`、`fail`。**缺任何一个，整个包判为不合格**，
不会出现在可选列表里 —— 否则会出现「失败配着笑脸」这种最致命的错配。

其余动作可缺，缺失时按链回退：

| 请求 | 回退顺序 |
|---|---|
| `hover` | hover → idle |
| `idle_blink` | idle_blink → idle |
| `running` | running → idle |
| `sleep` | sleep → idle |

组件通过 `supports(action)` 判断要不要播额外动画：包没提供 `idle_blink`，
就不播眨眼，而不是显示裂图。

## 制作素材

素材由 `buddy-forge` 产出（抠图 → 去溢色 → 脚底对齐 → WebP）：

```bash
cd .workbuddy/skills/buddy-forge
python scripts/forge.py pack --src raw/<name> --name <pack-id> --primary
```

`--src` 目录里的文件名必须正好是 `idle.png` / `success.png` / `fail.png` / `running.png`。
输出默认落到 `src/buddies/builtin/<pack-id>/`，跑完刷新页面即可看到。

关键前提：**素材必须是透明背景**。生图模型给的是不透明底色，
管线会用色键抠掉（键色从四角自动采样，因为模型不会老实给你 `#FF00FF`）。
透明的 alpha 轮廓还是 CSS 辉光的前提 —— 位图自带渐变底就做不出干净的发光边缘。

## 前端用法

```vue
<!-- 跟随全局设置 -->
<TaskBubbleGlow state="success" agent="Research Agent" task="生成趋势分析" />

<!-- 指定某套包 -->
<TaskBubbleGlow pack="robot" ... />
```

```ts
const { pack, packs, setPack, src, supports } = useBuddy();
```

## 数据流

```
Rust TaskNotice ──toBubbleView()──> BubbleView ──> 组件 props
                                        │
                                   useBuddy().src(state)
                                        │
                              pack.url() → 内置（打包路径）
                                          → 用户（asset 协议）
```

传输模型（`types/notice.ts`）和视图模型（`types/adapt.ts`）是分开的：
后端加字段不会逼着组件改，组件加视觉需求也不用动协议。
