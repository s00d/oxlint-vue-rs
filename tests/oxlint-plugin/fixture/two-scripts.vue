<script>
export default { inheritAttrs: false }
</script>
<script setup>
const props = defineProps({ v: Number })
const emit = defineEmits(['change'])
</script>
<template>
  <div :title="props.v == 2">{{props.v}}</div>
</template>
