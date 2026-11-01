<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  answeredFor,
  answerAsk,
  closeSheet,
  runAction,
  sheetNotice,
} from "../feed/notices";
import { toBubbleView } from "../types/adapt";
import { copyText, isOpenableUrl, looksLikePath, openExternal } from "../lib/open";
import MediaGallery from "./MediaGallery.vue";
import LinkText from "./LinkText.vue";

/**
 * 通知详情面板。
 *
 * 为什么需要它：气泡只有 400px 宽，结果摘要封顶两行，`detail`（日志尾部
 * 或结果链接）根本没地方放。之前「查看结果」只往服务端回一帧 action
 * 就把气泡收掉 —— 用户点了，内容没出现，那一帧也没人看见。
 *
 * 这个面板承担三件事：
 * 1. **把内容摆出来**：结果、详情、链接，能读能复制。
 * 2. **能真的打开**：链接交给系统默认程序（opener），不是只回传。
 * 3. **问询的作答现场**：问题 + 选项，选完把选项 id 作为 action 帧上行。
 */

const view = computed(() => {
  const n = sheetNotice.value;
  return n ? toBubbleView(n, answeredFor(n.id)) : null;
});

const accent = computed(() => view.value?.accent ?? ACCENT_OF(view.value?.state ?? "idle"));

function ACCENT_OF(state: string): string {
  switch (state) {
    case "success":
      return "#2FD9A4";
    case "running":
      return "#4DA6FF";
    case "fail":
      return "#FF4D5E";
    default:
      return "#8B93A7";
  }
}

const detail = computed(() => (view.value?.detail ?? "").trim());
/** 详情是一个地址时，展示成链接行而不是一段文本。 */
const urlTarget = computed(() => (isOpenableUrl(detail.value) ? detail.value : ""));
/**
 * 本机路径：允许交给系统打开（sys_open 侧把关存在性与可执行扩展名），
 * 但要留复制 —— 渲染节点上的产物路径对这台机器没意义，开了要明说。
 */
const pathTarget = computed(() =>
  !urlTarget.value && looksLikePath(detail.value) ? detail.value : ""
);

/** 打开一个本机路径。失败原因一句话摆出来，绝不静默。 */
async function openPath() {
  if (!pathTarget.value) return;
  const ok = await openExternal(pathTarget.value);
  flash.value = ok ? "已交给系统打开" : "打不开：文件可能不在这台机器上";
  window.setTimeout(() => (flash.value = ""), 2200);
}

const copied = ref(false);
const flash = ref("");
const sending = ref("");
const answered = computed(() => view.value?.answered ?? null);
/** 普通动作被拒的原因。和问询的 nack 分开：那个要把选项退回来，这个只说明一句。 */
const actionNack = computed(() => {
  const a = answered.value;
  if (!a || !a.failed || a.kind !== "action") return "";
  return a.reason || "没发出去";
});

watch(
  () => view.value?.task,
  () => {
    copied.value = false;
    flash.value = "";
    sending.value = "";
  }
);

async function openLink() {
  if (!urlTarget.value) return;
  const ok = await openExternal(urlTarget.value);
  flash.value = ok ? "已交给系统打开" : "打不开这个地址";
  window.setTimeout(() => (flash.value = ""), 1800);
}

async function copyDetail() {
  const text = detail.value || view.value?.result || "";
  if (!text) return;
  copied.value = await copyText(text);
  window.setTimeout(() => (copied.value = false), 1400);
}

async function pick(optionId: string) {
  if (!sheetNotice.value || sending.value) return;
  sending.value = optionId;
  const res = await answerAsk(sheetNotice.value.id, optionId);
  sending.value = "";
  if (!res.ok) flash.value = `没发出去：${res.reason ?? "连接断了"}`;
}

async function act(actionId: string) {
  if (!sheetNotice.value || sending.value) return;
  sending.value = actionId;
  const res = await runAction(sheetNotice.value.id, actionId);
  sending.value = "";
  if (!res.ok) flash.value = `没发出去：${res.reason ?? "连接断了"}`;
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && sheetNotice.value) {
    e.stopPropagation();
    closeSheet();
  }
}

onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <Transition name="veil">
      <div v-if="view" class="veil" @click="closeSheet" />
    </Transition>

    <Transition name="rise">
      <section
        v-if="view"
        class="sheet"
        :style="{ '--accent': accent }"
        role="dialog"
        aria-modal="true"
        aria-label="通知详情"
      >
        <header class="head">
          <span class="badge">
            <i class="dot" />
            {{ view.badge }}
          </span>
          <span class="who">{{ view.executor }}</span>
          <span class="time">{{ view.time }}</span>
          <button class="close" aria-label="关闭" @click="closeSheet">✕</button>
        </header>

        <h2 class="task">{{ view.task }}</h2>

        <p v-if="view.result" class="result">{{ view.result }}</p>

        <!-- 链接型详情：这是「查看结果」真正该打开的东西 -->
        <div v-if="urlTarget" class="link">
          <span class="link-label">结果地址</span>
          <code class="link-url" :title="urlTarget">{{ urlTarget }}</code>
          <span class="link-acts">
            <button class="mini primary" @click="openLink">打开</button>
            <button class="mini" @click="copyDetail">{{ copied ? "已复制" : "复制" }}</button>
          </span>
        </div>

        <!-- 本机路径：可交给系统打开（可执行文件由 Rust 侧拒绝），并留复制 -->
        <div v-else-if="pathTarget" class="link">
          <span class="link-label">产出路径</span>
          <code class="link-url" :title="pathTarget">{{ pathTarget }}</code>
          <span class="link-acts">
            <button class="mini primary" @click="openPath">打开</button>
            <button class="mini" @click="copyDetail">{{ copied ? "已复制" : "复制路径" }}</button>
          </span>
        </div>

        <!-- 文本型详情：日志尾部 / 长说明。可滚动，不裁成三行；链接可点 -->
        <pre v-else-if="detail" class="detail"><LinkText :text="detail" pre /></pre>

        <!-- 多媒体附件：图片网格 / 音视频播放器 / 文件卡片。
             running 状态也显示 —— 执行中先给封面是合理诉求 -->
        <MediaGallery v-if="view.media?.length" :media="view.media" />

        <!-- 没有正文时不必解释字段结构，用户只关心「还有没有更多可看」。
             问询类连这句都不用：问题本身就是全部内容。 -->
        <p v-else-if="!view.ask && !view.media?.length" class="none">
          这条通知只给了摘要，没有更多内容。
        </p>

        <!-- ── 问询 ─────────────────────────── -->
        <div v-if="view.ask" class="ask">
          <p class="ask-q">{{ view.ask.question }}</p>

          <!-- 已答复：只在真落地时显示。服务端拒了要退回选项让用户重选，
               否则这条就卡死在「已答复」，用户再没机会改。 -->
          <div v-if="answered && !answered.failed" class="answered" :class="{ wait: answered.unconfirmed }">
            <span class="tick">{{ answered.unconfirmed ? "?" : "✓" }}</span>
            <span v-if="answered.unconfirmed">
              已答复 · {{ answered.label }}
              <em class="sub">（服务端没有回执，这条可能没生效）</em>
            </span>
            <span v-else>已答复 · {{ answered.label }}</span>
          </div>

          <div v-else class="options">
            <p v-if="answered?.failed" class="nack">
              <span class="tick">!</span>
              <span>{{ answered.reason || "没发出去" }}，可以再选一次。</span>
            </p>
            <button
              v-for="o in view.ask.options"
              :key="o.id"
              class="option"
              :class="{ primary: o.primary }"
              :disabled="Boolean(sending)"
              @click="pick(o.id)"
            >
              {{ o.label }}
              <span v-if="sending === o.id" class="spin" aria-hidden="true" />
            </button>
          </div>
        </div>

        <!-- ── 普通动作 ─────────────────────── -->
        <!-- 动作被拒时把原因摆出来。按钮不撤：动作没有「选项」可退，
             能做的只有再点一次。 -->
        <p v-if="actionNack" class="nack">
          <span class="tick">!</span>
          <span>{{ actionNack }}，可以再点一次。</span>
        </p>
        <div v-if="view.actions.length && !view.ask" class="acts">
          <button
            v-for="a in view.actions"
            :key="a.id"
            class="option"
            :class="{ primary: a.primary }"
            :disabled="Boolean(sending)"
            @click="act(a.id)"
          >
            {{ a.label }}
            <span v-if="sending === a.id" class="spin" aria-hidden="true" />
          </button>
        </div>

        <footer class="foot">
          <span v-if="flash" class="flash">{{ flash }}</span>
          <span class="spring" />
          <button class="close-btn" @click="closeSheet">关闭</button>
        </footer>
      </section>
    </Transition>
  </Teleport>
</template>

<style scoped>
.veil {
  position: fixed;
  inset: 0;
  z-index: 70;
  background: rgba(4, 7, 13, 0.62);
  backdrop-filter: blur(4px);
}

.sheet {
  position: fixed;
  z-index: 71;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 520px;
  max-width: calc(100vw - 48px);
  max-height: min(80vh, 660px);
  display: flex;
  flex-direction: column;
  padding: 18px 20px 14px;
  border-radius: 18px;
  box-sizing: border-box;
  overflow: hidden;

  background:
    radial-gradient(120% 130% at 0% 0%, color-mix(in srgb, var(--accent) 16%, transparent) 0%, transparent 58%),
    linear-gradient(160deg, rgba(22, 33, 53, 0.98) 0%, rgba(10, 16, 28, 0.99) 100%);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 28%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    0 30px 80px -30px rgba(0, 0, 0, 0.95);
  color: #dce6f6;
}

.head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 9px 3px 7px;
  border-radius: 999px;
  font-size: 10.5px;
  letter-spacing: 0.03em;
  color: color-mix(in srgb, var(--accent) 88%, #ffffff);
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 34%, transparent);
}

.dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 8px var(--accent);
}

.who {
  font-size: 11.5px;
  color: #c3d0e4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time {
  margin-left: auto;
  font-size: 10.5px;
  color: #6f7f9b;
  font-variant-numeric: tabular-nums;
}

.close {
  width: 24px;
  height: 24px;
  flex: none;
  padding: 0;
  border: 0;
  border-radius: 7px;
  font-size: 11px;
  color: #7d8ca6;
  background: transparent;
  cursor: pointer;
}

.close:hover {
  color: #eaf2ff;
  background: rgba(255, 255, 255, 0.08);
}

.task {
  margin: 12px 0 0;
  font-size: 16px;
  font-weight: 600;
  line-height: 1.35;
  letter-spacing: 0.02em;
  color: #f3f8ff;
}

.result {
  margin: 7px 0 0;
  font-size: 12.5px;
  line-height: 1.6;
  color: #b9c7dc;
}

/* 文本详情：等宽、可滚、保留换行。日志缩进是信息的一部分 */
.detail {
  margin: 12px 0 0;
  padding: 11px 13px;
  max-height: 220px;
  overflow: auto;
  border-radius: 10px;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 11px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
  color: #a8b8d0;
  background: rgba(0, 0, 0, 0.32);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
}

.none {
  margin: 12px 0 0;
  font-size: 11px;
  line-height: 1.6;
  color: #5d6c85;
}

.link {
  margin-top: 12px;
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 6px 10px;
  padding: 10px 12px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.035);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
}

.link-label {
  grid-column: 1 / -1;
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: #5d6c85;
}

.link-url {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  align-self: center;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 11px;
  color: #8fd8ee;
}

.link-acts {
  display: flex;
  gap: 6px;
}

.mini {
  font: inherit;
  font-size: 11px;
  padding: 4px 10px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 999px;
  background: transparent;
  color: #a7b6cd;
  cursor: pointer;
}

.mini:hover {
  color: #eaf2ff;
  background: rgba(255, 255, 255, 0.07);
}

.mini.primary {
  border-color: transparent;
  color: #051019;
  font-weight: 600;
  background: linear-gradient(180deg, #9defff, #58b6f5);
}

/* ── 问询 ───────────────────────────────── */
.ask {
  margin-top: 14px;
  padding-top: 13px;
  border-top: 1px solid rgba(255, 255, 255, 0.07);
}

.ask-q {
  margin: 0 0 10px;
  font-size: 13px;
  line-height: 1.55;
  color: #eaf2ff;
}

.options {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.option {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font: inherit;
  font-size: 12px;
  padding: 8px 16px;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: rgba(255, 255, 255, 0.04);
  color: #c3d0e4;
  cursor: pointer;
  transition: background 140ms ease, border-color 140ms ease, color 140ms ease;
}

.option:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.09);
  color: #eaf2ff;
}

.option:disabled {
  opacity: 0.55;
  cursor: progress;
}

/* 主选项跟着状态色走：这是一个决定，不是普通链接 */
.option.primary {
  border-color: color-mix(in srgb, var(--accent) 55%, transparent);
  color: #04101a;
  font-weight: 600;
  background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 80%, #ffffff), var(--accent));
  box-shadow: 0 6px 18px -8px color-mix(in srgb, var(--accent) 85%, transparent);
}

.option.primary:hover:not(:disabled) {
  filter: brightness(1.06);
  color: #04101a;
}

.answered {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 12px;
  border-radius: 10px;
  font-size: 12px;
  color: #7ff0c4;
  background: rgba(47, 217, 164, 0.09);
  box-shadow: inset 0 0 0 1px rgba(47, 217, 164, 0.3);
}

/* 「发出去了，但对面没回执」。用琥珀而不是绿：绿是「这件事办成了」，
   而这里只办成了一半 —— 帧出去了，没人承认收到。 */
.answered.wait {
  color: #e8c07a;
  background: rgba(224, 160, 48, 0.09);
  box-shadow: inset 0 0 0 1px rgba(224, 160, 48, 0.3);
}

.answered.wait .tick {
  color: #2a1d05;
  background: #e8c07a;
}

.answered .sub {
  font-style: normal;
  opacity: 0.75;
}

/* 答复没落地。放在选项上方而不是替掉选项 —— 用户要的是「重选一次」，
   不是被告知失败然后无处可点。 */
.nack {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  /* 独占一行。跟按钮挤在同一行时，警告会被按钮吃掉一半宽度，
     读起来像按钮的附注，而它其实是「刚才那次没成」的正文。 */
  flex: 0 0 100%;
  margin: 0 0 8px;
  padding: 8px 11px;
  border-radius: 10px;
  font-size: 12px;
  line-height: 1.5;
  color: #ff9aa5;
  background: rgba(255, 77, 94, 0.09);
  box-shadow: inset 0 0 0 1px rgba(255, 77, 94, 0.32);
}

.nack .tick {
  margin-top: 2px;
  color: #2a0b0f;
  background: #ff9aa5;
}

/* 圆底和字形必须两个颜色。之前写成 `background: currentColor`，
   等于把字和底涂成同一个色 —— 那个「✓」画出来了，但看不见。 */
.tick {
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  flex: none;
  border-radius: 50%;
  font-size: 10px;
  color: #04120d;
  background: #7ff0c4;
}

.acts {
  margin-top: 12px;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.foot {
  margin-top: 14px;
  padding-top: 11px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  display: flex;
  align-items: center;
  gap: 10px;
}

.spring {
  flex: 1 1 auto;
}

.flash {
  font-size: 11px;
  color: #f2b34b;
}

.close-btn {
  font: inherit;
  font-size: 11.5px;
  padding: 6px 14px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 9px;
  background: transparent;
  color: #9daec8;
  cursor: pointer;
}

.close-btn:hover {
  color: #eaf2ff;
  background: rgba(255, 255, 255, 0.07);
}

/* 正在发送：一个小转圈，避免用户以为点空了又点一次 */
.spin {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  border: 1.5px solid currentColor;
  border-top-color: transparent;
  animation: spin 620ms linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.veil-enter-active,
.veil-leave-active {
  transition: opacity 160ms ease;
}

.veil-enter-from,
.veil-leave-to {
  opacity: 0;
}

.rise-enter-active {
  transition: opacity 180ms ease, transform 220ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

.rise-leave-active {
  transition: opacity 140ms ease, transform 160ms ease;
}

.rise-enter-from,
.rise-leave-to {
  opacity: 0;
  transform: translate(-50%, -46%) scale(0.98);
}

@media (prefers-reduced-motion: reduce) {
  .rise-enter-active,
  .rise-leave-active,
  .veil-enter-active,
  .veil-leave-active {
    transition-duration: 1ms;
  }
  .spin {
    animation: none;
  }
}
</style>
