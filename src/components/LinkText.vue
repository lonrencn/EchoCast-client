<script setup lang="ts">
/**
 * 可点击链接的文本渲染器。
 *
 * 服务端在 result / detail 里夹的 URL（裸链接或 `[文案](url)`）画成锚点，
 * 点击交给系统默认程序（lib/open.ts 白名单：http/https/mailto）。
 * 纯文本原样渲染 —— 不用 v-html，服务端内容永远不进 innerHTML。
 *
 * 注意父容器往往有点击语义（气泡卡片点击 = 收起），所以锚点必须
 * .stop.prevent，点链接不能把气泡点没了。
 */
import { computed } from "vue";
import { hasLink, linkify } from "../lib/linkify";
import { openExternal } from "../lib/open";

const props = defineProps<{
  text: string;
  /** pre = 保留换行缩进（面板里的日志详情用） */
  pre?: boolean;
}>();

const segs = computed(() => (hasLink(props.text) ? linkify(props.text) : null));

async function onLink(ev: MouseEvent, url: string) {
  ev.stopPropagation();
  ev.preventDefault();
  await openExternal(url);
}
</script>

<template>
  <span v-if="!segs">{{ text }}</span>
  <span v-else :class="{ pre }"
    ><template v-for="(s, i) in segs" :key="i"
      ><a
        v-if="s.kind === 'link'"
        :href="s.value"
        class="tlink"
        :title="s.value"
        @click.stop.prevent="onLink($event, s.value)"
        >{{ s.label ?? s.value }}</a
      ><template v-else>{{ s.value }}</template></template
    ></span
  >
</template>

<style scoped>
.tlink {
  color: var(--accent, #4fd6c3);
  text-decoration: underline;
  text-underline-offset: 2px;
  text-decoration-color: color-mix(in srgb, currentColor 55%, transparent);
  cursor: pointer;
  word-break: break-all;
}

.tlink:hover {
  text-decoration-color: currentColor;
}

.pre {
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
