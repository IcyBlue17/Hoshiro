<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import Pill from "./Pill.vue";
import Kbd from "./Kbd.vue";

type Tok = { t: string; drop?: boolean };
type Scene = {
  id: string;
  tab: string;
  keys: string[];
  app: "mail" | "chat" | "doc" | "ask";
  title: string;
  spoken: Tok[];
  result: string;
  chip?: string;
  ask?: boolean;
};

const SCENES: Scene[] = [
  {
    id: "dictate", tab: "口述", keys: ["fn"], app: "mail", title: "邮件 — 会议改期",
    spoken: [
      { t: "嗯，", drop: true }, { t: "那个，", drop: true }, { t: "跟大家说一下，" },
      { t: "周三的会，", drop: true }, { t: "啊不对，", drop: true },
      { t: "周四下午三点开会，" }, { t: "然后", drop: true }, { t: "记得带上 Q3 的报告" },
    ],
    result: "跟大家说一下，周四下午 3 点开会，记得带上 Q3 的报告。",
  },
  {
    id: "translate", tab: "翻译", keys: ["fn", "⇧"], app: "chat", title: "# design-team",
    chip: "English",
    spoken: [{ t: "嗯，", drop: true }, { t: "明天上午我们一起" }, { t: "就是，", drop: true }, { t: "过一下新版的设计稿吧" }],
    result: "Let's go over the new design drafts together tomorrow morning.",
  },
  {
    id: "rewrite", tab: "改写", keys: ["fn", "Space"], app: "doc", title: "回复.txt", ask: true,
    spoken: [{ t: "帮我改得" }, { t: "嗯，", drop: true }, { t: "礼貌专业一点" }],
    result: "这个需求目前实现成本较高，我们评估后暂时无法排期，感谢理解。",
  },
  {
    id: "ask", tab: "问答", keys: ["fn", "Space"], app: "ask", title: "Safari", ask: true,
    spoken: [{ t: "用一句话" }, { t: "呃，", drop: true }, { t: "解释一下 Rust 的所有权" }],
    result: "每个值都有唯一的所有者，所有者离开作用域时值会被自动释放——编译期检查，零运行时开销。",
  },
];

const idx = ref(0);
const scene = computed(() => SCENES[idx.value]);
const phase = ref<"ready" | "recording" | "processing" | "done">("ready");
const spokenCount = ref(0);
const elapsed = ref(0);
const progress = ref(0);
const visible = ref(true);
let token = 0;
let rafId = 0;

const sleep = (ms: number, my: number) =>
  new Promise<void>((res, rej) => setTimeout(() => (my === token ? res() : rej("cancel")), ms));

async function play(i: number) {
  const my = ++token;
  idx.value = i;
  phase.value = "ready";
  spokenCount.value = 0;
  elapsed.value = 0;
  const s = SCENES[i];
  const total = 1100 + s.spoken.length * 520 + 300 + 1300 + 3200;
  const start = performance.now();
  cancelAnimationFrame(rafId);
  const tick = () => {
    progress.value = Math.min(1, (performance.now() - start) / total);
    if (my === token) rafId = requestAnimationFrame(tick);
  };
  tick();
  try {
    await sleep(1100, my);
    phase.value = "recording";
    const recStart = performance.now();
    for (let k = 1; k <= s.spoken.length; k++) {
      await sleep(520, my);
      spokenCount.value = k;
      elapsed.value = Math.floor((performance.now() - recStart) / 1000);
    }
    await sleep(300, my);
    phase.value = "processing";
    await sleep(1300, my);
    phase.value = "done";
    await sleep(3200, my);
    // pause cycling while off-screen
    while (!visible.value) await sleep(400, my);
    play((i + 1) % SCENES.length);
  } catch {
    /* superseded */
  }
}

let io: IntersectionObserver | undefined;
const root = ref<HTMLElement>();
onMounted(() => {
  io = new IntersectionObserver(([e]) => (visible.value = e.isIntersecting), { threshold: 0.1 });
  if (root.value) io.observe(root.value);
  play(0);
});
onBeforeUnmount(() => {
  token++;
  cancelAnimationFrame(rafId);
  io?.disconnect();
});

const keysDown = computed(() => phase.value === "recording");
const pillState = computed(() =>
  phase.value === "recording" ? "recording" : phase.value === "processing" ? "processing" : "idle",
);
const showResult = computed(() => phase.value === "done");
const elapsedLabel = computed(() => `0:${String(elapsed.value).padStart(2, "0")}`);
</script>

<template>
  <div ref="root" class="hd">
    <div class="hd-tabs" role="tablist">
      <button
        v-for="(s, i) in SCENES" :key="s.id" role="tab" class="hd-tab" :class="{ active: i === idx }"
        :aria-selected="i === idx" @click="play(i)"
      >
        {{ s.tab }}<span class="hd-tab-keys">{{ s.keys.join(" + ") }}</span>
        <span v-if="i === idx" class="hd-tab-bar"><span :style="{ transform: `scaleX(${progress})` }" /></span>
      </button>
    </div>

    <div class="hd-stage">
      <div class="hd-window">
        <div class="hd-chrome">
          <span class="tl" /><span class="tl" /><span class="tl" />
          <span class="hd-title">{{ scene.title }}</span>
        </div>

        <Transition name="hd-fade" mode="out-in">
          <!-- Mail -->
          <div v-if="scene.app === 'mail'" key="mail" class="hd-body">
            <div class="hd-field"><span>收件人</span>team@studio.dev</div>
            <div class="hd-field"><span>主题</span>会议改期</div>
            <div class="hd-text">
              <template v-if="showResult"><span class="hd-ins">{{ scene.result }}</span></template>
              <span class="hd-caret" />
            </div>
          </div>

          <!-- Chat -->
          <div v-else-if="scene.app === 'chat'" key="chat" class="hd-body hd-chat">
            <div class="hd-msg"><b class="av">A</b><div><i>Alex</i>New mockups are up on Figma.</div></div>
            <div class="hd-msg"><b class="av">M</b><div><i>Mia</i>Looks great! When can we review?</div></div>
            <div class="hd-msg mine" :class="{ show: showResult }"><b class="av">我</b><div><i>你</i>{{ scene.result }}</div></div>
            <div class="hd-input">
              <template v-if="!showResult"><span class="hd-ph">发送消息到 #design-team</span></template>
              <span class="hd-caret" />
            </div>
          </div>

          <!-- Rewrite selection -->
          <div v-else-if="scene.app === 'doc'" key="doc" class="hd-body">
            <div class="hd-text">
              <p class="hd-muted">Hi，关于上周提的导出 PDF 需求：</p>
              <p>
                <span v-if="!showResult" class="hd-sel">这个功能我们做不了，太麻烦了。</span>
                <span v-else class="hd-ins">{{ scene.result }}</span>
                <span class="hd-caret" />
              </p>
            </div>
          </div>

          <!-- Ask -->
          <div v-else key="ask" class="hd-body hd-browser">
            <div class="hd-url">doc.rust-lang.org/book</div>
            <div class="hd-skel"><i style="width:62%" /><i style="width:90%" /><i style="width:78%" /><i style="width:84%" /><i style="width:40%" /></div>
          </div>
        </Transition>
      </div>

      <!-- floating stack above the pill -->
      <div class="hd-float">
        <Transition name="hd-pop">
          <div v-if="scene.chip && phase !== 'ready' && phase !== 'done'" class="hd-chip">→ {{ scene.chip }}</div>
        </Transition>
        <Transition name="hd-pop">
          <div v-if="scene.app === 'ask' && showResult" class="hd-answer">
            <div class="hd-answer-h">回答</div>
            <div class="hd-answer-q">用一句话解释一下 Rust 的所有权</div>
            <div class="hd-answer-b">{{ scene.result }}</div>
          </div>
        </Transition>
      </div>
      <div class="hd-pill">
        <Pill :state="pillState" :ask="scene.ask && phase !== 'ready' && phase !== 'done'" :elapsed="elapsedLabel" />
      </div>

      <div class="hd-keys" :class="{ down: keysDown }">
        <Kbd v-for="k in scene.keys" :key="k" :pressed="keysDown" size="sm">{{ k }}</Kbd>
        <span class="hd-keys-hint">{{ phase === "recording" ? "录音中" : phase === "processing" ? "识别与润色中" : "" }}</span>
      </div>
    </div>

    <div class="hd-said" :class="{ live: phase === 'recording', cleaned: phase === 'processing' || phase === 'done' }">
      <span class="hd-said-label">原始语音</span>
      <span class="hd-said-text">
        <span v-if="spokenCount === 0" class="hd-said-ph">长按快捷键说话，转录与过滤显示在此处</span>
        <span v-for="(t, i) in scene.spoken.slice(0, spokenCount)" :key="scene.id + i" class="tok" :class="{ drop: t.drop }">{{ t.t }}</span>
      </span>
    </div>
  </div>
</template>
