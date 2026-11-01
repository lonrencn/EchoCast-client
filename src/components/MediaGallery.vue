<script setup lang="ts">
/**
 * 媒体附件展示区。
 *
 * 下载走 Rust 的 media_fetch（带缓存），前端只拿本地路径再用
 * convertFileSrc 转成 asset:// 给 <img>/<video>/<audio>。
 * 为什么不用 WebView 直接 fetch：媒体服务几乎一定与 WS 不同源，
 * CORS 把命运交到服务端手里；Rust 请求不受它管，还能复用「接受自签证书」。
 *
 * 浏览器预览（非 Tauri）没有 IPC，直接把原始 url 塞给标签 ——
 * 演示环境能看到效果，真下载路径只有桌面端走。
 */
import { onBeforeUnmount, onMounted, reactive, ref } from "vue";
import type { MediaItem } from "../types/notice";
import { settings } from "../settings/useSettings";

const props = defineProps<{ media: MediaItem[] }>();

type State = { status: "loading" | "ok" | "error"; path?: string; size?: number; msg?: string };

const states = reactive(new Map<string, State>());

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function stateOf(url: string): State {
  return (
    states.get(url) ?? { status: inTauri ? "loading" : "ok", path: inTauri ? undefined : url }
  );
}

async function fetchOne(item: MediaItem) {
  if (!inTauri) {
    states.set(item.url, { status: "ok", path: item.url });
    return;
  }
  states.set(item.url, { status: "loading" });
  try {
    const { invoke, convertFileSrc } = await import("@tauri-apps/api/core");
    const res = await invoke<{ path: string; size: number; cached: boolean }>("media_fetch", {
      url: item.url,
      token: settings.value.token || null,
    });
    states.set(item.url, {
      status: "ok",
      path: convertFileSrc(res.path),
      size: res.size,
    });
  } catch (e) {
    states.set(item.url, { status: "error", msg: String(e) });
  }
}

function retry(item: MediaItem) {
  void fetchOne(item);
}

onMounted(() => {
  // 并发拉全部：Rust 侧有 URL 级缓存，重复推同一条通知不会重复下载
  for (const m of props.media) void fetchOne(m);
});

onBeforeUnmount(() => states.clear());

/** 放大查看的图片。同屏只放大一张，再点一次收起。 */
const expanded = ref<string | null>(null);

function toggleExpand(url: string) {
  expanded.value = expanded.value === url ? null : url;
}

const ICONS: Record<string, string> = { image: "🖼", audio: "🎧", video: "🎬", file: "📄" };
const LABELS: Record<string, string> = {
  image: "图片",
  audio: "音频",
  video: "视频",
  file: "文件",
};

function fmtSize(bytes?: number): string {
  if (!bytes || bytes <= 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function openFile(item: MediaItem) {
  const st = states.get(item.url);
  if (!st?.path) return;
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    await invoke("media_open", { path: st.path });
  } catch (e) {
    console.warn("[media] 打开失败:", e);
  }
}

const images = () => props.media.filter((m) => m.kind === "image");
const others = () => props.media.filter((m) => m.kind !== "image");
</script>

<template>
  <div class="gallery">
    <!-- 图片：缩略图网格，点开放大 -->
    <div v-if="images().length" class="img-grid" :class="{ single: images().length === 1 }">
      <figure
        v-for="img in images()"
        :key="img.url"
        class="img-cell"
        :class="{ open: expanded === img.url }"
        @click="stateOf(img.url).status === 'ok' && toggleExpand(img.url)"
      >
        <template v-if="stateOf(img.url).status === 'loading'">
          <span class="ph shimmer" />
        </template>
        <template v-else-if="stateOf(img.url).status === 'error'">
          <div class="ph fail" @click.stop="retry(img)">
            <span>图片加载失败</span>
            <button class="retry">重试</button>
          </div>
        </template>
        <img
          v-else
          :src="stateOf(img.url).path"
          :alt="img.name || '图片'"
          draggable="false"
          loading="lazy"
        />
        <figcaption v-if="expanded !== img.url && (img.name || fmtSize(stateOf(img.url).size))">
          <span class="img-name">{{ img.name || LABELS.image }}</span>
          <span v-if="fmtSize(stateOf(img.url).size)" class="img-size">{{
            fmtSize(stateOf(img.url).size)
          }}</span>
        </figcaption>
      </figure>
    </div>

    <!-- 音频 / 视频 / 其他文件 -->
    <div v-for="m in others()" :key="m.url" class="media-row" :data-kind="m.kind">
      <template v-if="stateOf(m.url).status === 'loading'">
        <span class="row-icon">{{ ICONS[m.kind] }}</span>
        <div class="row-main">
          <span class="row-name">{{ m.name || LABELS[m.kind] }}</span>
          <span class="row-meta shimmer line" />
        </div>
      </template>

      <template v-else-if="stateOf(m.url).status === 'error'">
        <span class="row-icon bad">{{ ICONS[m.kind] }}</span>
        <div class="row-main">
          <span class="row-name">{{ m.name || LABELS[m.kind] }}</span>
          <span class="row-meta err">下载失败 · {{ stateOf(m.url).msg }}</span>
        </div>
        <button class="mini" @click="retry(m)">重试</button>
      </template>

      <template v-else>
        <span class="row-icon">{{ ICONS[m.kind] }}</span>
        <div class="row-main">
          <span class="row-name">{{ m.name || LABELS[m.kind] }}</span>
          <span v-if="m.kind === 'audio'" class="row-meta">音频 · 点播放试听</span>
          <video
            v-else-if="m.kind === 'video'"
            class="player"
            controls
            preload="metadata"
            :poster="m.poster"
            :src="stateOf(m.url).path"
          />
          <template v-else>
            <span class="row-meta">
              {{ LABELS.file }}<template v-if="fmtSize(stateOf(m.url).size)">
                · {{ fmtSize(stateOf(m.url).size) }}</template
              >
            </span>
          </template>
        </div>
        <button
          v-if="m.kind === 'file'"
          class="mini primary"
          @click="openFile(m)"
        >
          打开
        </button>
      </template>

      <!-- 音频播放器独占一行 -->
      <audio
        v-if="m.kind === 'audio' && stateOf(m.url).status === 'ok'"
        class="audio"
        controls
        preload="metadata"
        :src="stateOf(m.url).path"
      />
    </div>
  </div>
</template>

<style scoped>
.gallery {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* ── 图片网格 ─────────────────────────────── */
.img-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 8px;
}

.img-grid.single {
  grid-template-columns: 1fr;
}

.img-cell {
  position: relative;
  margin: 0;
  border-radius: 12px;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.03);
  cursor: zoom-in;
  min-height: 90px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.img-cell.open {
  grid-column: 1 / -1;
  cursor: zoom-out;
}

.img-cell img {
  display: block;
  width: 100%;
  max-height: 130px;
  object-fit: cover;
}

.img-cell.open img {
  max-height: 420px;
  object-fit: contain;
}

.img-cell figcaption {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 9px;
  font-size: 10px;
  color: #cbd7ea;
  background: linear-gradient(transparent, rgba(4, 8, 16, 0.82));
  pointer-events: none;
}

.img-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.img-size {
  flex: none;
  color: #8b9ab4;
  font-variant-numeric: tabular-nums;
}

.ph {
  display: block;
  width: 100%;
  height: 90px;
}

.shimmer {
  background: linear-gradient(100deg, rgba(255, 255, 255, 0.04) 40%, rgba(255, 255, 255, 0.09) 50%, rgba(255, 255, 255, 0.04) 60%);
  background-size: 200% 100%;
  animation: sh 1.4s ease-in-out infinite;
}

@keyframes sh {
  to {
    background-position: -200% 0;
  }
}

.ph.fail {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 7px;
  font-size: 11px;
  color: #8b9ab4;
  cursor: default;
}

.retry,
.mini {
  font: inherit;
  font-size: 10.5px;
  padding: 4px 12px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: transparent;
  color: #9daec8;
  cursor: pointer;
  transition: background 140ms ease, color 140ms ease;
}

.retry:hover,
.mini:hover {
  background: rgba(255, 255, 255, 0.07);
  color: #eaf2ff;
}

.mini.primary {
  flex: none;
  border: 0;
  color: #06121d;
  font-weight: 600;
  background: color-mix(in srgb, var(--accent, #2fd9a4) 88%, #ffffff);
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent, #2fd9a4) 40%, transparent);
}

/* ── 音频 / 视频 / 文件行 ─────────────────── */
.media-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.025);
}

.row-icon {
  flex: none;
  font-size: 16px;
  line-height: 1;
}

.row-icon.bad {
  filter: grayscale(1) brightness(0.7);
}

.row-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.row-name {
  font-size: 12px;
  color: #dbe6f6;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-meta {
  font-size: 10.5px;
  color: #5d6c85;
}

.row-meta.err {
  color: #ff9aa5;
}

.row-meta.line {
  height: 10px;
  width: 120px;
  border-radius: 5px;
}

.player {
  width: 100%;
  max-height: 240px;
  border-radius: 8px;
  background: #000;
}

.audio {
  flex: 1 1 100%;
  height: 34px;
}

/* 原生控件的深色化：只调得动 WebKit */
.audio::-webkit-media-controls-panel {
  background: rgba(20, 30, 50, 0.9);
}
</style>
