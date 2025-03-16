<template lang="pug">

  .acceptor

    button.button.accept(@click="$emit('accept')")

      Icon(icon="check-circle" fixed-width)

      | Keep

    button.button.reject(@click="$emit('reject')")

      Icon(icon="undo" fixed-width)

      | Reset

</template>


<style lang="stylus" scoped>

  .acceptor
    pointer-events: auto
    display: flex

  .button
    margin: 0
    border-radius: 0
    flex: 1 1 auto
    text-align: center

    & + .button
      border-left: none

    .button + &
      border-right: none

    &:first-child
      border-top-left-radius: 4px
      border-bottom-left-radius: 4px

    &:last-child
      border-top-right-radius: 4px
      border-bottom-right-radius: 4px

    svg
      padding: 0.25rem

  .accept:hover svg
    color: $confirm

  .reject:hover svg
    color: $cancel

</style>


<script setup>

  const emit = defineEmits(['accept', 'reject'])

</script>
