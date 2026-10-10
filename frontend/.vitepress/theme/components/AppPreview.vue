<script setup lang="ts">
// The real hub UI (src/hub), built by `bun run app` into public/app and running on the browser mock backend.
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useData, withBase } from "vitepress";

const W = 1180; // logical size the hub is laid out at
const H = 780;
const MIN_WIDTH = 720; // below this the scaled UI is too small to use; show a still instead

const { isDark } = useData();
const base = withBase("/app/index.html?shot");
const src = ref(base);
const frame = ref<HTMLIFrameElement>();
const box = ref<HTMLElement>();
const width = ref(0);
const touched = ref(false);
const scale = computed(() => Math.min(1, width.value / W));
const usable = computed(() => width.value >= MIN_WIDTH);
let ro: ResizeObserver | undefined;

onMounted(() => {
  src.value = `${base}&appearance=${isDark.value ? "dark" : "light"}`;
  ro = new ResizeObserver(([e]) => (width.value = e.contentRect.width));
  ro.observe(box.value!);
});
onBeforeUnmount(() => ro?.disconnect());

// Follow the site's light/dark toggle without reloading the demo.
watch(isDark, (dark) => {
  frame.value?.contentWindow?.postMessage({ appearance: dark ? "dark" : "light" }, location.origin);
});
</script>

<template>
  <div class="ap">
    <div
      ref="box" class="ap-viewport" :style="{ height: usable ? H * scale + 'px' : 'auto' }"
      @pointerenter="touched = true"
    >
      <iframe
        v-if="usable" ref="frame" :src="src" title="Hoshiro 界面演示"
        :style="{ width: W + 'px', height: H + 'px', transform: `scale(${scale})` }"
      />
      <a v-else-if="width" :href="src" target="_blank" rel="noreferrer" class="ap-still">
        <img :src="withBase('/images/home.png')" alt="Hoshiro 主界面" />
      </a>
      <div v-if="usable" class="ap-hint" :class="{ gone: touched }">可直接点击 · 试试左侧导航和设置</div>
    </div>
    <p v-if="width && !usable" class="ap-note">
      当前视口较小，建议 <a :href="src" target="_blank" rel="noreferrer">在新标签页中打开交互预览 →</a>
    </p>
  </div>
</template>
