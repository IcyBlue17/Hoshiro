<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

type Tok = { t: string; k?: "filler" | "fix" };
type Ex = { label: string; desc: string; before: Tok[]; after: string; list?: string[] };

const EXAMPLES: Ex[] = [
  {
    label: "语气词过滤",
    desc: "移除“嗯、啊、那个”等口癖与无意义停顿。",
    before: [
      { t: "嗯，", k: "filler" }, { t: "我觉得" }, { t: "那个，", k: "filler" },
      { t: "这个方案" }, { t: "就是，", k: "filler" }, { t: "整体上是可行的，" },
      { t: "啊，", k: "filler" }, { t: "但预算还得再看看。" },
    ],
    after: "我觉得这个方案整体上是可行的，但预算还得再看看。",
  },
  {
    label: "改口纠偏",
    desc: "识别口述中的中途纠错，保留最终意图。",
    before: [
      { t: "我们约" }, { t: "周三，", k: "fix" }, { t: "不对，", k: "filler" }, { t: "周四" },
      { t: "晚上七点，", k: "fix" }, { t: "哦改成", k: "filler" }, { t: "八点吃饭吧" },
    ],
    after: "我们约周四晚上八点吃饭吧。",
  },
  {
    label: "结构化列表",
    desc: "口述枚举条目自动排版为编号列表。",
    before: [
      { t: "下周要做三件事，" }, { t: "第一", k: "filler" }, { t: "是上线新版本，" },
      { t: "第二", k: "filler" }, { t: "是写发布文档，" }, { t: "第三呢", k: "filler" }, { t: "是回复用户反馈" },
    ],
    after: "下周要做三件事：",
    list: ["上线新版本", "写发布文档", "回复用户反馈"],
  },
  {
    label: "混排规范",
    desc: "规范阿拉伯数字与中英文字符间盘空格排版。",
    before: [{ t: "帮我在iPhone十五上" }, { t: "装一下" }, { t: "TestFlight的" }, { t: "第三个构建版本" }],
    after: "帮我在 iPhone 15 上装一下 TestFlight 的第 3 个构建版本。",
  },
];

const i = ref(0);
const step = ref(0); // 0: raw, 1: marked, 2: cleaned
const ex = computed(() => EXAMPLES[i.value]);
const root = ref<HTMLElement>();
let timers: number[] = [];
let inView = false;
let io: IntersectionObserver | undefined;

function clear() { timers.forEach(clearTimeout); timers = []; }
function run(n: number) {
  clear();
  i.value = n;
  step.value = 0;
  timers.push(window.setTimeout(() => (step.value = 1), 900));
  timers.push(window.setTimeout(() => (step.value = 2), 1900));
  timers.push(window.setTimeout(() => { if (inView) run((n + 1) % EXAMPLES.length); }, 6200));
}

onMounted(() => {
  io = new IntersectionObserver(([e]) => {
    const was = inView;
    inView = e.isIntersecting;
    if (inView && !was) run(i.value);
    if (!inView) clear();
  }, { threshold: 0.3 });
  io.observe(root.value!);
});
onBeforeUnmount(() => { clear(); io?.disconnect(); });
</script>

<template>
  <div ref="root" class="cd">
    <div class="cd-tabs">
      <button v-for="(e, n) in EXAMPLES" :key="e.label" :class="{ active: n === i }" @click="run(n)">{{ e.label }}</button>
    </div>
    <p class="cd-desc">{{ ex.desc }}</p>
    <div class="cd-rows">
      <div class="cd-row">
        <div class="cd-tag">原始口述</div>
        <p class="cd-raw" :class="{ marked: step >= 1 }">
          <span v-for="(t, n) in ex.before" :key="i + '-' + n" :class="t.k">{{ t.t }}</span>
        </p>
      </div>
      <div class="cd-row">
        <div class="cd-tag">润色输出</div>
        <Transition name="hd-fade" mode="out-in">
          <div v-if="step >= 2" :key="i" class="cd-out">
            <p>{{ ex.after }}</p>
            <ol v-if="ex.list"><li v-for="l in ex.list" :key="l">{{ l }}</li></ol>
          </div>
          <div v-else class="cd-wait"><i /><i /><i /></div>
        </Transition>
      </div>
    </div>
  </div>
</template>
