<script setup lang="ts">
/**
 * 消息回看（主窗口）。
 *
 * 托盘「消息回看」进来这里是历史列表：以任务名 + 时间为索引，
 * 每行显示最新状态与摘要。点一条 = 把历史快照整体发给悬浮层，
 * 原样弹回桌面 —— 气泡、问询选项、链接、答复回执全走既有管线，
 * 这里不做第二套交互。弹完把主窗口收起来，让用户直接看到气泡。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  clearHistory,
  historyList,
  reloadHistory,
  type HistoryEntry,
} from "../feed/notices";

const emit = defineEmits<{ back: [] }>();

const query = ref("");

/** 按任务名过滤（子串，大小写不敏感）。时间倒序由 historyList 保证。 */
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return historyList.value;
  return historyList.value.filter((e) =>
    e.notice.task.toLowerCase().includes(q)
  );
});

const STATUS: Record<string, { label: string; color: string }> = {
  success: { label: "完成", color: "#2FD9A4" },
  running: { label: "执行中", color: "#4DA6FF" },
  failed: { label: "失败", color: "#FF4D5E" },
  warning: { label: "注意", color: "#F5A524" },
  ask: { label: "待答复", color: "#B48CFF" },
  idle: { label: "通知", color: "#8B93A7" },
};

function statusOf(e: HistoryEntry) {
  return STATUS[e.notice.status] ?? STATUS.idle;
}

/** 今天/昨天/日期 + 时分。回看的索引一半是时间，得一眼能认出是哪天。 */
function fmtTime(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (d.toDateString() === now.toDateString()) return `今天 ${hm}`;
  const yest = new Date(now);
  yest.setDate(now.getDate() - 1);
  if (d.toDateString() === yest.toDateString()) return `昨天 ${hm}`;
  return `${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${hm}`;
}

/** 摘要：结果一行，没有就用详情首行。 */
function excerpt(e: HistoryEntry): string {
  const t = (e.notice.result || e.notice.detail || "").replace(/\s+/g, " ").trim();
  return t.length > 64 ? `${t.slice(0, 64)}…` : t;
}

/** 点一条：快照发给悬浮层原样弹出，主窗口收起让位。 */
async function replay(e: HistoryEntry) {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit("ec:notice-replay", e);
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().hide();
  } catch (err) {
    console.warn("[history] 回放失败", err);
  }
}

async function wipe() {
  clearHistory();
  reloadHistory();
}

function onStorage(ev: StorageEvent) {
  if (ev.key === "echocast.history") reloadHistory();
}

onMounted(() => {
  reloadHistory(); // 悬浮层可能刚写过历史，打开时重读
  window.addEventListener("storage", onStorage);
});
onBeforeUnmount(() => window.removeEventListener("storage", onStorage));
</script>

<template>
  <div class="page">
    <header class="head">
      <button class="back" title="返回设置" @click="emit('back')">
        <svg viewBox="0 0 14 14" width="13" height="13" aria-hidden="true">
          <path
            d="M11.5 7h-8M6.6 3.6 3.2 7l3.4 3.4"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        设置
      </button>
      <h1>消息回看</h1>
      <button class="wipe" :disabled="!historyList.length" @click="wipe">清空记录</button>
    </header>

    <div class="search">
      <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
        <circle cx="7" cy="7" r="4.4" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path d="m10.4 10.4 3 3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
      </svg>
      <input
        v-model="query"
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder="按任务名搜索…"
      />
      <span class="count">{{ filtered.length }} 条</span>
    </div>

    <div class="list">
      <button
        v-for="e in filtered"
        :key="e.notice.id"
        class="row"
        :title="'点击在桌面原样弹出这条通知'"
        @click="replay(e)"
      >
        <span class="dot" :style="{ background: statusOf(e).color }" />
        <span class="mid">
          <span class="line1">
            <b>{{ e.notice.task }}</b>
            <i class="chip" :style="{ color: statusOf(e).color }">{{ statusOf(e).label }}</i>
            <i v-if="e.answered" class="chip done">已答复 · {{ e.answered.label }}</i>
          </span>
          <span v-if="excerpt(e)" class="line2">{{ excerpt(e) }}</span>
        </span>
        <span class="time">{{ fmtTime(e.notice.timestamp) }}</span>
      </button>

      <p v-if="!filtered.length" class="empty">
        {{ query ? "没有匹配的任务" : "还没有历史通知 —— 收起过的通知都会留在这里" }}
      </p>
      <p v-else class="hint">点任意一条，通知会按原样弹回桌面，未答复的问询可以继续作答。</p>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 14px;
  height: 100vh;
  padding: 28px 34px;
  overflow: hidden;
}

.head {
  display: flex;
  align-items: center;
  gap: 14px;
}

.head h1 {
  flex: 1;
  margin: 0;
  font-family: "Noto Serif SC", serif;
  font-size: 19px;
  font-weight: 600;
  letter-spacing: 0.04em;
  color: #eef4ff;
}

.back,
.wipe {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  font: inherit;
  font-size: 12px;
  border-radius: 999px;
  cursor: pointer;
  color: #b9c6dd;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  transition: background 140ms ease, color 140ms ease;
}

.back:hover,
.wipe:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: #eef4ff;
}

.wipe:disabled {
  opacity: 0.4;
  cursor: default;
}

.search {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 0 14px;
  height: 38px;
  border-radius: 12px;
  color: #6b7a94;
  background: rgba(255, 255, 255, 0.045);
  border: 1px solid rgba(255, 255, 255, 0.09);
}

.search input {
  flex: 1;
  min-width: 0;
  font: inherit;
  font-size: 12.5px;
  color: #e8eef9;
  background: transparent;
  border: 0;
  outline: none;
}

.search input::placeholder {
  color: #5d6c85;
}

.count {
  flex: none;
  font-size: 11px;
  color: #6b7a94;
}

.list {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-y: auto;
  padding-right: 4px;
}

.row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 15px;
  font: inherit;
  text-align: left;
  border-radius: 14px;
  cursor: pointer;
  background: rgba(255, 255, 255, 0.035);
  border: 1px solid rgba(255, 255, 255, 0.08);
  transition: background 140ms ease, border-color 140ms ease;
}

.row:hover {
  background: rgba(255, 255, 255, 0.07);
  border-color: rgba(255, 255, 255, 0.16);
}

.dot {
  width: 8px;
  height: 8px;
  flex: none;
  border-radius: 50%;
  box-shadow: 0 0 8px currentColor;
}

.mid {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.line1 {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.line1 b {
  font-size: 13px;
  font-weight: 600;
  color: #eef4ff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.chip {
  flex: none;
  padding: 1px 8px;
  border-radius: 999px;
  font-size: 10px;
  font-style: normal;
  letter-spacing: 0.04em;
  border: 1px solid currentColor;
  opacity: 0.9;
}

.chip.done {
  color: #7ee2c8;
}

.line2 {
  font-size: 11.5px;
  color: #8494ae;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.time {
  flex: none;
  font-size: 11px;
  color: #6b7a94;
  font-variant-numeric: tabular-nums;
}

.empty {
  margin: 40px 0;
  text-align: center;
  font-size: 13px;
  color: #5d6c85;
}

.hint {
  margin: 6px 0 2px;
  text-align: center;
  font-size: 11.5px;
  color: #5d6c85;
}
</style>
