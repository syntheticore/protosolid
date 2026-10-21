<template lang="pug">
  .selector-widget(
    :style="{top: widget.pos.y + 'px', left: widget.pos.x + 'px'}"
    @pointerdown.stop
  )
    .panel
      .actions.bordered.tipped
        button(@click="back")
          Icon(icon="angle-left")

        button.confirm(@click="select")
          Icon(icon="check-circle")

        button(@click="forward")
          Icon(icon="angle-right")

      .info
        .count {{ index + 1 }} / {{ widget.items.length }} Elements
        .type {{ choice.typename() }}

</template>


<style lang="stylus" scoped>
  @import '../styles/selector-widget.styl'

  .panel
    transform: translateX(-50%)
    display: flex
    flex-direction: column
    align-items: center

  .actions.bordered
    &::before
      left: 50%
      margin-left: -7px

  .confirm
    color: $confirm

  .info
    margin-top: 6px
    padding: 3px 7px
    border-radius: 3px
    background: rgba(black, 0.4)
    text-align: center
    font-size: 11px
    line-height: 1.35

  .count
    color: $bright2 * 1.2

  .type
    color: $bright1
    font-weight: 600

</style>


<script>
  export default {
    name: 'SelectorWidget',

    inject: ['bus'],

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
      this.bus.on('close-widgets', this.remove)
      this.$emit('change', this.widget.items[0])
    },

    beforeUnmount() {
      this.bus.off('close-widgets', this.remove)
    },

    methods: {
      back() {
        this.index--
        if(this.index < 0) this.index = this.widget.items.length - 1
        this.$emit('change', this.widget.items[this.index])
      },

      forward() {
        this.index++
        if(this.index >= this.widget.items.length) this.index = 0
        this.$emit('change', this.widget.items[this.index])
      },

      select() {
        this.widget.cb(this.choice)
        this.$emit('remove')
      },

      remove() {
        this.$emit('remove')
      }
    },
  }
</script>
