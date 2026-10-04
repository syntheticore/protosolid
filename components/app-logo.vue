<template lang="pug">
  img.app-logo(:src="logoUrl" alt="ProtoSolid" draggable="false")
</template>

<style lang="stylus" scoped>
  .app-logo
    display: inline-block
    width: 1.25em
    height: 1em
    object-fit: contain
    vertical-align: middle
    flex-shrink: 0
</style>

<script setup>
  import logoUrl from '../electron/build/icon-source.png'
</script>
