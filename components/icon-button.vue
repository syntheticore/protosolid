<template lang="pug">

  button.button.icon-button

    Icon(:icon="icon" fixed-width :class="{ margin: !!$slots.default }")

    slot

</template>


<style lang="stylus" scoped>

  svg.margin
    margin-right: 0.5rem

</style>


<script setup>

  const props = defineProps(['icon'])

</script>
