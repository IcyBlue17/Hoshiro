<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const TEXT = "周五前把新版官网上线，首页加上动态演示，教程按安装、配置、进阶三块拆开写，顺便把常见问题整理一下，下周一给大家过一遍。";
const SPEAK = 230; // 字/分钟（首页统计里的平均口述速度）
const TYPE = 55;   // 字/分钟（常见中文打字速度）
const LOOP = 12; // 秒

const t = ref(0);
const root = ref<HTMLElement>();
let raf = 0, start = 0, io: IntersectionObserver | undefined, running = false;

function frame(now: number) {
  t.value = ((now - start) / 1000) % LOOP;
  raf = requestAnimationFrame(frame);
}
onMounted(() => {
  io = new IntersectionObserver(([e]) => {
    if (e.isIntersecting && !running) { running = true; start = performance.now() - t.value * 1000; raf = requestAnimationFrame(frame); }
    if (!e.isIntersecting && running) { running = false; cancelAnimationFrame(raf); }
  }, { threshold: 0.3 });
  io.observe(root.value!);
});
onBeforeUnmount(() => { cancelAnimationFrame(raf); io?.disconnect(); });

// 演示时间 ×6 加速，让 12 秒能看完整段
const speed = 6;
const spoken = computed(() => Math.min(TEXT.length, Math.floor((t.value * speed * SPEAK) / 60)));
const typed = computed(() => Math.min(TEXT.length, Math.floor((t.value * speed * TYPE) / 60)));
</script>

<template>
  <div ref="root" class="sr">
    <div class="sr-lane">
      <div class="sr-head"><span>打字</span><b>~{{ TYPE }} 字/分钟</b></div>
      <div class="sr-bar"><span :style="{ width: (typed / TEXT.length) * 100 + '%' }" /></div>
      <p class="sr-text"><span>{{ TEXT.slice(0, typed) }}</span><i class="sr-caret" /><span class="sr-ghost">{{ TEXT.slice(typed) }}</span></p>
    </div>
    <div class="sr-lane sr-voice">
      <div class="sr-head"><span>口述</span><b>~{{ SPEAK }} 字/分钟</b></div>
      <div class="sr-bar"><span :style="{ width: (spoken / TEXT.length) * 100 + '%' }" /></div>
      <p class="sr-text"><span>{{ TEXT.slice(0, spoken) }}</span><i v-if="spoken < TEXT.length" class="sr-caret" /><span class="sr-ghost">{{ TEXT.slice(spoken) }}</span><em v-if="spoken >= TEXT.length" class="sr-done">完成</em></p>
    </div>
  </div>
</template>
