import type { TaskNotice } from "../types/notice";

/**
 * 浏览器里的 mock 数据源。
 *
 * 存在的唯一理由：`pnpm dev` 在浏览器里没有 Tauri IPC。
 * 场景内容与 `src-tauri/src/mock.rs` 保持一致，这样两边看到的行为是同一套。
 */

type Push = (n: TaskNotice) => void;

interface Scenario {
  task: string;
  executor: TaskNotice["executor"];
  status: TaskNotice["status"];
  ticking: string[];
  result: string;
  actions: NonNullable<TaskNotice["actions"]>;
  /** 需要人答复时的问题与选项。有它这条就是问询。 */
  ask?: TaskNotice["ask"];
}

const SCENARIOS: Scenario[] = [
  {
    task: "生成 Q3 市场趋势分析",
    executor: { name: "Research Agent", kind: "agent" },
    status: "success",
    ticking: ["抓取 42 个数据源", "清洗与对齐口径", "生成图表 18 张", "校对引用"],
    result: "报告已生成，共 26 页",
    actions: [
      { id: "open", label: "查看结果", style: "primary" },
      { id: "copy", label: "复制路径", style: "ghost" },
    ],
  },
  {
    task: "方言保留微调 · 第 3 轮",
    executor: { name: "GPU 节点 02", node: "ACE-Step", kind: "remote" },
    status: "warning",
    ticking: ["加载语料 12.4 GB", "第 3 轮反向传播", "校验音质", "导出权重"],
    result: "显存峰值 23.8 / 24 GB，batch 降到 4 可重试",
    // 这一条是问询：要不要继续属于人的判断，不是自动重试能决定的
    actions: [],
    ask: {
      question: "显存峰值 23.8 / 24 GB，已经贴顶。降 batch 重跑，还是今晚先停？",
      options: [
        { id: "retry-low", label: "降到 batch 4 重跑", style: "primary" },
        { id: "hold", label: "今晚先停", style: "ghost" },
      ],
    },
  },
  {
    task: "合成《传承的力量》第 4 段",
    executor: { name: "Soundao", node: "Edge TTS", kind: "local" },
    status: "success",
    ticking: ["切分 6 个语段", "合成第 1–2 段", "合成第 3–4 段", "混流与响度归一"],
    result: "1 分 42 秒，响度 -16 LUFS",
    actions: [{ id: "open", label: "试听", style: "primary" }],
  },
  {
    task: "检查异常登录",
    executor: { name: "Security Agent", kind: "agent" },
    status: "failed",
    ticking: ["拉取近 24h 鉴权日志", "比对设备指纹", "关联地理跳变", "生成处置建议"],
    result: "检测到非常规活动，已阻断",
    actions: [
      { id: "open", label: "查看详情", style: "primary" },
      { id: "ignore", label: "忽略", style: "ghost" },
    ],
  },
];

function tickingText(s: Scenario, progress: number): string {
  if (!s.ticking.length) return "";
  const i = Math.min(s.ticking.length - 1, Math.floor(progress * s.ticking.length));
  return s.ticking[i];
}

function compose(s: Scenario, id: string, status: TaskNotice["status"], progress?: number): TaskNotice {
  const running = status === "running";
  return {
    id,
    task: s.task,
    executor: s.executor,
    status,
    result: running ? tickingText(s, progress ?? 0) : s.result,
    progress,
    // 执行中不给动作：这时候点「重试」只会重复触发一次任务
    actions: running ? [] : s.actions,
    // 执行中还没到「等你答复」那一步，别提前催
    ask: running ? undefined : s.ask,
    timestamp: Date.now(),
  };
}

/** 每个任务走一遍「执行中 → 终态」，然后换下一个场景。 */
export function startBrowserMock(push: Push, seed = 0) {
  let round = seed;
  let timer: number | undefined;

  const runOne = () => {
    const s = SCENARIOS[round % SCENARIOS.length];
    // id 跟着场景走，不跟轮次走：同一任务再跑一次是更新，不是新起一条
    const id = `mock-${round % SCENARIOS.length}`;
    const steps = [0.12, 0.34, 0.58, 0.81];
    let i = 0;

    const tick = () => {
      if (i < steps.length) {
        push(compose(s, id, "running", steps[i]));
        i += 1;
        timer = window.setTimeout(tick, 720);
        return;
      }
      push(compose(s, id, s.status));
      round += 1;
      timer = window.setTimeout(runOne, 4600);
    };

    tick();
  };

  // 留一点时间让窗口和订阅先就绪
  timer = window.setTimeout(runOne, 1200);

  return () => {
    if (timer !== undefined) clearTimeout(timer);
  };
}

/** 手动推一条问询。演示用：按一下就听到 dididi、看到「等你答复…」。 */
export function pushBrowserAsk(push: Push) {
  const s = SCENARIOS[1];
  push(compose(s, "mock-ask-demo", s.status));
}

/** 手动推一条终态通知。 */
export function pushBrowserOnce(push: Push) {
  const round = Math.floor(Date.now() / 1000);
  const s = SCENARIOS[round % SCENARIOS.length];
  push(compose(s, `mock-manual-${round}`, s.status));
}
