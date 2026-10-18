<template lang="pug">
  .selector-widget(
    :style="{top: widget.pos.y + 'px', left: widget.pos.x + 'px'}"
    @pointerdown.stop
  )
    .panel
      .actions
        button.button(@click="back")
          Icon(icon="angle-left")

        button.button.confirm(@click="select")
          Icon(icon="check-circle")

        button.button(@click="forward")
          Icon(icon="angle-right")

      .info
        .count {{ index + 1 }} / {{ widget.items.length }} Elements
        .type {{ choice.typename() }}

</template>


<style lang="stylus" scoped>
  .panel
    width: max-content
    transform: translateX(-50%)
    padding-top: 8px
    display: flex
    flex-direction: column
    align-items: center

  .actions
    display: flex
    border-radius: 5px
    box-shadow: 0 0 0 1px $dark2 * 0.55, 0 1px 4px rgba(black, 0.15)

  .button
    margin: 0
    width: 32px
    padding: 3px 0
    display: flex
    justify-content: center
    box-shadow: none
    border-radius: 0
    &:first-child
      border-radius: 5px 0 0 5px
    &:last-child
      border-radius: 0 5px 5px 0
    & + .button
      border-left-color: $dark2 * 0.8

  // Arrow is part of the confirm button, so it shares its fill and top edge
  .confirm
    position: relative
    color: $confirm
    &::before
      content: ''
      position: absolute
      top: -7px
      left: 50%
      margin-left: -7px
      border-left: 7px solid transparent
      border-right: 7px solid transparent
      border-bottom: 7px solid $dark1 * 1.2
      filter: drop-shadow(0 -1px 0px $dark1 * 1.5) drop-shadow(0 -1px 0px $dark2 * 0.55)
    &:hover::before
      filter: drop-shadow(0 -1px 0px $dark1 * 1.7) drop-shadow(0 -1px 0px $dark2 * 0.55)
    &:active::before
      border-bottom-color: $dark1 * 1.1
      filter: drop-shadow(0 -1px 0px $dark1 * 0.95) drop-shadow(0 -1px 0px $dark2 * 0.55)

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
