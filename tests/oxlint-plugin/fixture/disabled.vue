<template>
  <!-- eslint-disable-next-line vue-rs/require-v-for-key -->
  <i v-for="x in xs">{{ x }}</i>
  <!-- eslint-disable vue-rs/html-self-closing -->
  <b></b>
  <!-- eslint-enable vue-rs/html-self-closing -->
  <u></u>
       <s>bad indent</s>
</template>
<script>
export default {
  // eslint-disable-next-line vue-rs/order-in-components
  data() { return {} },
  name: 'Disabled',
  // oxlint-disable-next-line vue-rs/order-in-components
  props: {},
}
</script>
