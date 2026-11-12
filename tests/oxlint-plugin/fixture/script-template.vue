<template>
  <div>
    <span v-for="x in list">{{x}}</span>
    <button @click="$emit('save')">ok</button>
  </div>
</template>
<script>
export default {
  data() { return { list: [] } },
  props: { a: String },
  name: 'ScriptTemplate',
}
</script>
<style>
.a { color: red }
</style>
