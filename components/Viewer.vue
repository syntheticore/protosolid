<template lang="pug">

  .viewer
    //- model-viewer(v-if="url" :src="url" camera-controls)

</template>


<script setup>
  // import "@google/model-viewer/dist/model-viewer";

  const props = defineProps(['url'])

</script>


<style lang="stylus" scoped>

  .viewer
    height: 100vh

  model-viewer
    width: 100% !important
    height: 100% !important

</style>
