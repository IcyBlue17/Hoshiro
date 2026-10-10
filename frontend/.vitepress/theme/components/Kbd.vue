<script setup lang="ts">
defineProps<{ pressed?: boolean; size?: "sm" | "md" | "lg" }>();
</script>

<template>
  <kbd class="hs-kbd" :class="[size ? 'hs-kbd-' + size : '', { pressed }]"><slot /></kbd>
</template>
