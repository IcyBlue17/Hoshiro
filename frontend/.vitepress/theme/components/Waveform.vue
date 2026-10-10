<script setup lang="ts">
// Port of src/bar/Waveform.tsx, with a built-in fake "speech" level instead of the mic.
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = withDefaults(defineProps<{ mode: "live" | "processing" | "idle"; color?: string }>(), {
  color: "255,255,255",
});

const N = 11, BAR_W = 3, GAP = 3, H = 24, MIN_H = 3, MAX_H = 20, LERP = 0.3;
const W = N * BAR_W + (N - 1) * GAP;
const canvas = ref<HTMLCanvasElement>();
let raf = 0;

// Fake speech: syllable bursts separated by short pauses.
let speaking = true, until = 0, amp = 0.5, t0 = 0;
function fakeLevel(now: number) {
  if (now > until) {
    speaking = !speaking;
    until = now + (speaking ? 700 + Math.random() * 1600 : 250 + Math.random() * 450);
    amp = 0.35 + Math.random() * 0.6;
  }
  if (!speaking) return 0.006;
  const syll = Math.abs(Math.sin(((now - t0) / 1000) * Math.PI * 4.2));
  return Math.min(1, amp * Math.pow(syll, 0.7) * (0.75 + Math.random() * 0.25) + Math.random() * 0.04);
}

watch(() => props.mode, (m) => {
  if (m === "live") { speaking = true; until = performance.now() + 1200; t0 = performance.now(); }
});

onMounted(() => {
  const c = canvas.value!;
  const ctx = c.getContext("2d")!;
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
  const heights = new Float32Array(N).fill(MIN_H);
  const f1 = Array.from({ length: N }, () => 4 + Math.random() * 4);
  const f2 = Array.from({ length: N }, () => 9 + Math.random() * 6);
  const p1 = Array.from({ length: N }, () => Math.random() * 6.283);
  const p2 = Array.from({ length: N }, () => Math.random() * 6.283);
  const bell = Array.from({ length: N }, (_, i) => Math.exp(-(((i - (N - 1) / 2) / 3.4) ** 2)));
  let env = 0, dpr = 0, prev = performance.now(), level = 0, lastSample = 0;
  t0 = prev; until = prev + 1200;

  const fit = () => {
    const d = window.devicePixelRatio || 1;
    if (d === dpr) return;
    dpr = d;
    c.width = Math.round(W * d);
    c.height = Math.round(H * d);
    ctx.setTransform(d, 0, 0, d, 0, 0);
  };
  const pill = (x: number, y: number, w: number, h: number) => {
    const r = Math.min(w, h) / 2;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  };

  const frame = (now: number) => {
    raf = requestAnimationFrame(frame);
    fit();
    const dt = Math.min(64, now - prev);
    prev = now;
    const k60 = dt / 16.667;
    const m = props.mode;
    const t = now / 1000;
    if (now - lastSample > 33) { level = fakeLevel(now); lastSample = now; }

    let input = 0;
    if (m === "live") input = level < 0.01 ? 0 : Math.pow(Math.min(1, level), 0.6);
    const a = input > env ? 0.6 : 0.16;
    env += (input - env) * (1 - Math.pow(1 - a, k60));
    const kLerp = 1 - Math.pow(1 - LERP, k60);
    const still = reduce.matches;
    const quiet = 1 - Math.min(1, env * 8);

    ctx.clearRect(0, 0, W, H);
    for (let i = 0; i < N; i++) {
      const w = 0.3 + 0.7 * bell[i];
      let target = MIN_H, alpha = 1;
      if (m === "live") {
        const noise = 0.5 + 0.25 * Math.sin(t * f1[i] + p1[i]) + 0.25 * Math.sin(t * f2[i] + p2[i]);
        const speech = Math.min(1, env * w * (0.58 + 0.42 * noise) * 1.3);
        const breath = still ? 0 : quiet * (0.5 + 0.5 * Math.sin(t * 2.4 - i * 0.55)) * (0.35 + 0.65 * bell[i]) * 2.4;
        target = MIN_H + (MAX_H - MIN_H) * speech + breath;
      } else if (m === "processing") {
        alpha = still ? 0.8 : 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * 5.5 - i * 0.65));
      } else {
        alpha = 0.6;
      }
      heights[i] += (target - heights[i]) * kLerp;
      const h = heights[i];
      ctx.fillStyle = `rgba(${props.color},${alpha.toFixed(3)})`;
      pill(i * (BAR_W + GAP), (H - h) / 2, BAR_W, h);
      ctx.fill();
    }
  };
  fit();
  raf = requestAnimationFrame(frame);
});
onBeforeUnmount(() => cancelAnimationFrame(raf));
</script>

<template>
  <canvas ref="canvas" class="hs-waveform" :style="{ width: W + 'px', height: H + 'px', display: 'block' }" />
</template>
