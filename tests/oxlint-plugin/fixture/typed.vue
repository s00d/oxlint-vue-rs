<script setup lang="ts">
const emit = defineEmits<{ (e: 'go', id: number): void }>()
const props = defineProps<{ n: number }>()
const count = ref<number>(props.n as number)
</script>

<template>
  <div v-for="i in (count as number)" :key="i">{{i}}</div>
  <a @click="emit('stop')">ü — 😀</a>
</template>
