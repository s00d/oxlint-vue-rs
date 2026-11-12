<template>
    <ul>
      <li v-for="item in items" class="b" :id="item">{{item}}</li>
      <MyInput></MyInput>
      <p v-if="a == 1">😀 ü — émoji before {{a}}</p>
    </ul>
</template>
