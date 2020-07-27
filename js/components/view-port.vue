<template lang="pug">
  .view-port
    canvas
    ul.widgets
      li(
        v-for="widget in widgets"
        :style="{top: widget.pos.y + 'px', left: widget.pos.x + 'px'}"
     )
</template>


<style lang="stylus" scoped>
  $color = desaturate($highlight, 35%)
  .view-port
    position: relative
    overflow: hidden
    border-top: 1px solid $color * 0.375

  canvas
    display: block
    background: $color * 0.2
    background: radial-gradient(50% 150%, farthest-corner, $color * 0.35, $color * 0.2)

  .widgets
    position: absolute
    left: 0
    right: 0
    top: 0
    bottom: 0
    pointer-events: none
    li
      size = 29px
      padding: 6px
      position: absolute
      display: block
      width: size
      height: size
      margin-left: -(size / 2)
      margin-top: -(size / 2)
      pointer-events: auto
      // cursor: move
      &::after
        display: block
        content: ''
        margin: 0
        padding: 0
        width:  calc(100% - 4px)
        height: calc(100% - 4px)
        border-radius: 99px
        border: 2px solid #85de85
        transition: all 0.2s
        opacity: 0
        transform: scale(1.5)
        pointer-events: none
      &:hover
        &::after
          transform: scale(1)
          opacity: 1
</style>


<script>
  import * as THREE from 'three'
  import { Renderer } from './../renderer.js'

  var renderer

  export default {
    name: 'ViewPort',

    props: {
      tree: Object,
    },

    watch: {
      tree: function() {
        renderer.loadTree(this.tree)
      },
    },

    data() {
      return {
        widgets: [],
      }
    },

    mounted: function() {
      this.renderer = new Renderer(this.$el.querySelector('canvas'))

      this.renderer.on('render', () => {
        this.updateWidgets()
      })

      this.renderer.on('change-view', () => {
        this.$emit('change-view')
      })

      this.renderer.on('change-pose', () => {
        this.$emit('change-pose')
      })

      this.renderer.loadTree(this.tree)
    },

    beforeDestroy: function() {
      this.renderer.dispose()
    },

    methods: {
      updateWidgets: function() {
        if(!this.renderer.handles) return
        this.widgets.length = 0
        this.renderer.handles.forEach((point, i) => {
          const pos = this.renderer.toScreen(new THREE.Vector3().fromArray(point))
          this.$set(this.widgets, i, {pos, type: 'vertex'})
        })
      },
    }
  }
</script>
