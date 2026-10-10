<script setup lang="ts">
// Replica of the floating bar pill (src/bar/bar.css). `cycle` loops idle → recording → processing on its own.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import Waveform from "./Waveform.vue";

type State = "idle" | "recording" | "processing";
const props = withDefaults(defineProps<{ state?: State; ask?: boolean; cycle?: boolean; elapsed?: string }>(), {
  state: "idle",
});

const own = ref<State>(props.cycle ? "idle" : props.state);
const current = computed<State>(() => (props.cycle ? own.value : props.state));

let timer = 0;
onMounted(() => {
  if (!props.cycle) return;
  const seq: [State, number][] = [["idle", 1400], ["recording", 3600], ["processing", 1600]];
  let i = 0;
  const step = () => {
    own.value = seq[i][0];
    timer = window.setTimeout(step, seq[i][1]);
    i = (i + 1) % seq.length;
  };
  step();
});
onBeforeUnmount(() => clearTimeout(timer));
</script>

<template>
  <span class="hs-pill-host">
    <span class="hs-pill" :class="['s-' + current, { ask }]">
      <span class="hs-pill-glow" />
      <span class="hs-pill-clip"><span class="hs-shimmer" :class="{ on: current === 'processing' }" /></span>
      <span class="hs-wave" :class="{ off: current === 'idle' }">
        <Waveform :mode="current === 'recording' ? 'live' : current === 'processing' ? 'processing' : 'idle'" />
      </span>
      <span v-if="elapsed && current === 'recording'" class="hs-elapsed">{{ elapsed }}</span>
    </span>
  </span>
</template>
