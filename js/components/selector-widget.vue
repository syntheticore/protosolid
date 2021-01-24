<template lang="pug">
  .selector-widget.tipped(
    :style="{top: widget.pos.y + 'px', left: widget.pos.x + 'px'}"
    @pointerdown.stop
  )

    .actions
      button.button(@click="back")
        fa-icon(icon="angle-left")

      button.button(@click="select")
        fa-icon(icon="check-circle")

      button.button(@click="forward")
        fa-icon(icon="angle-right")

    .info
      span {{ widget.items.length }} Elements

</template>


<style lang="stylus" scoped>
  .selector-widget
    padding-top: 8px
    margin-left: -38px
    &::before
      left: 32px
      top: 2px
      z-index: 1

  .actions
    display: flex
    box-shadow: 0 1px 4px rgba(black, 0.15)

  .info
    text-align: center
    font-size: 11px
  span
    display: inline-block
    margin-top: 6px
    padding: 3px 5px
    border-radius: 2px
    background: rgba(black, 0.4)
    color: $bright2 * 1.2

  .button
    margin: 0
    padding: 3px 8px
    &:not(:first-child):not(:last-child)
      z-index: 1
      border-radius: 0
      color: $confirm

</style>


<script>
  export default {
    name: 'SelectorWidget',

    props: {
      widget: Object,
    },

    data() {
      return {
        index: 0,
      }
    },

    computed: {
      choice: function() {
        return this.widget.items[this.index]
      },
    },

    mounted() {
      this.$root.$on('close-widgets', () => this.$emit('remove') )
    },

    methods: {
      back: function() {
        this.index--
        if(this.index < 0) this.index = this.widget.items.length - 1
          console.log(this.widget.items[this.index])
        this.$emit('change', this.widget.items[this.index])
      },

      forward: function() {
        this.index++
        if(this.index >= this.widget.items.length) this.index = 0
        this.$emit('change', this.widget.items[this.index])
      },

      select: function() {
        this.widget.cb(this.choice)
        this.$emit('remove')
      },
    },
  }
</script>
