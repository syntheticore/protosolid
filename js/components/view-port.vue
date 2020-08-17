<template lang="pug">
  .view-port
    canvas(
      ref="canvas"
      @mousedown="mouseDown"
      @mousemove="mouseMove"
    )
    ul.widgets
      li(
        v-for="widget in widgets"
        @click="widgetClicked(widget)"
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
      size = 21px
      // padding: 7px
      position: absolute
      display: block
      width: size
      height: size
      margin-left: -(size / 2)
      margin-top: -(size / 2)
      pointer-events: auto
      // cursor: move
      display: flex
      align-items: center
      justify-content: center
      &::before
        position: absolute
        display: block
        content: ''
        margin: 0
        padding: 0
        width:  7px
        height: 7px
        border-radius: 99px
        background: #ffefae
      &::after
        position: absolute
        display: block
        content: ''
        margin: 0
        padding: 0
        width:  calc(100% - 4px)
        height: calc(100% - 4px)
        border-radius: 99px
        border: 2px solid $highlight * 1.6
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
  import { LineTool } from './../tools.js'

  function getMouseCoords(e, canvas) {
    var coords = new THREE.Vector2()
    var rect = e.target.getBoundingClientRect();
    coords.x = (e.clientX - rect.left) / canvas.offsetWidth * 2 - 1
    coords.y = - (e.clientY - rect.top) / canvas.offsetHeight * 2 + 1
    return coords
  }

  export default {
    name: 'ViewPort',

    props: {
      tree: Object,
      activeComponent: Object,
      activeTool: Object,
    },

    watch: {
      tree: function() {
        this.renderer.loadTree(this.tree)
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

      this.renderer.on('component-changed', (comp) => {
        this.renderer.loadTree(this.tree)
      })

      this.$root.$on('activate-toolname', (toolName) => {
        let tool
        switch(toolName) {
          case 'Line':
          tool = new LineTool(this.activeComponent, this.renderer)
          break;
        }
        this.$emit('activate-tool', tool)
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
          const vec = new THREE.Vector3().fromArray(point)
          const pos = this.renderer.toScreen(vec)
          this.$set(this.widgets, i, {pos, vec, type: 'vertex'})
        })
      },

      mouseDown: function(e) {
        const coords = getMouseCoords(e, this.$refs.canvas)
        this.renderer.onMouseDown(coords)
        const vec = this.renderer.fromScreen(coords)
        if(this.activeTool && vec) this.activeTool.mouseDown(vec)
      },

      mouseMove: function(e) {
        const coords = getMouseCoords(e, this.$refs.canvas)
        this.renderer.onMouseMove(coords)
        if(!this.activeTool) return
        const vec = this.renderer.fromScreen(coords)
        if(vec) this.activeTool.mouseMove(vec)
      },

      widgetClicked: function(widget) {
        if(this.activeTool) this.activeTool.mouseDown(widget.vec)
      },
    }
  }
</script>
