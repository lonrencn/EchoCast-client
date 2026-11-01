<script setup lang="ts">
import { computed } from "vue";
import TaskBubbleGlow from "./TaskBubbleGlow.vue";
import { toBubbleView } from "../types/adapt";
import {
  answeredFor,
  armReadFade,
  answerAsk,
  dismiss,
  expanded,
  holdReadFade,
  openSheet,
  overflow,
  runAction,
  toggleExpanded,
  visible,
} from "../feed/notices";

const items = computed(() =>
  visible.value.map((n) => ({
    id: n.id,
    view: toBubbleView(n, answeredFor(n.id)),
  }))
);
</script>

<template>
  <!-- 右下角浮层。现在挂在主窗口内，稳定后再拆成独立 always-on-top 窗口 -->
  <div class="stack" aria-live="polite">
    <TransitionGroup name="pop">
      <div v-for="it in items" :key="it.id" class="slot">
        <TaskBubbleGlow
          v-bind="it.view"
          @action="(a: string) => runAction(it.id, a)"
          @answer="(oid: string) => answerAsk(it.id, oid)"
          @dismiss="dismiss(it.id)"
          @read-fade="(on: boolean) => on && armReadFade(it.id)"
          @hold-fade="holdReadFade(it.id)"
          @open-sheet="openSheet(it.id)"
        />
      </div>
    </TransitionGroup>

    <!-- 被挤出的未读通知不丢：点开全部滚出，一条条看完 -->
    <button v-if="overflow > 0" class="more" @click="toggleExpanded">
      还有 {{ overflow }} 条未响应 · 点击查看
    </button>
    <button v-else-if="expanded" class="more" @click="toggleExpanded">收起</button>
  </div>
</template>

<style scoped>
.stack {
  position: fixed;
  right: 18px;
  bottom: 18px;
  z-index: 50;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 14px;
  /* 窗口空白区不能吃掉下面页面的点击 */
  pointer-events: none;
}

.slot {
  pointer-events: auto;
}

.more {
  font: inherit;
  font-size: 11px;
  letter-spacing: 0.03em;
  color: #6c7c98;
  padding: 5px 12px;
  border-radius: 999px;
  background: rgba(10, 16, 28, 0.72);
  border: 1px solid rgba(255, 255, 255, 0.07);
  cursor: pointer;
  pointer-events: auto;
  transition: color 140ms ease, border-color 140ms ease;
}

.more:hover {
  color: #9fe8cd;
  border-color: rgba(47, 217, 164, 0.35);
}

/* 入场比退场慢：浮上来要有分量，离开要利落 */
.pop-enter-active {
  transition:
    opacity 180ms cubic-bezier(0.22, 0.61, 0.36, 1),
    transform 180ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

.pop-leave-active {
  position: absolute;
  right: 0;
  transition:
    opacity 160ms ease-in,
    transform 160ms ease-in;
}

.pop-enter-from {
  opacity: 0;
  transform: translateY(8px);
}

.pop-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

/* 被顶上去的旧气泡要滑，不是瞬移 */
.pop-move {
  transition: transform 260ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

@media (prefers-reduced-motion: reduce) {
  .pop-enter-active,
  .pop-leave-active,
  .pop-move {
    transition-duration: 1ms;
  }
}
</style>
