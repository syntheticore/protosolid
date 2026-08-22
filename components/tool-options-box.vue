<template lang="pug">

  .bordered.tool-options-box

    .main

      .settings

        template(v-for="(setting, key) in activeTool.settings" :key="key")

          .setting(v-if="!setting.visible || setting.visible(activeTool)")

            span {{ setting.title }}

            RadioBar(
              v-if="setting.type == 'select'"
              :items="setting.options"
              :chosen="activeTool[key]"
              @update:chosen="setSetting(key, $event)"
            )

            IconToggle(
              v-if="setting.type == 'bool'"
              :icons="setting.icons"
              :active="activeTool[key]"
              @update:active="setSetting(key, $event)"
            )

            select.input.enum-input(
              v-if="setting.type == 'enum'"
              :value="activeTool[key]"
              @change="setSetting(key, $event.target.value)"
            )
              option(v-for="(name, option) in setting.options" :key="option" :value="option") {{ name }}

            IntegerInput(
              v-if="setting.type == 'integer'"
              :value="activeTool[key]"
              :min="setting.min"
              :max="setting.max"
              @update:value="setSetting(key, $event)"
            )

            ScalarInput(
              v-if="setting.type == 'number'"
              :value="activeTool[key]"
              :min="setting.min"
              :max="setting.max"
              :step="setting.step || 1"
              @update:value="setSetting(key, $event)"
            )

    .confirmation

      button.cancel(title="Cancel" @click="close")
        Icon(icon="times-circle")

</template>


<style lang="stylus" scoped>

  .tool-options-box
    font-size: 12px
    display: flex
    &::before
      left: 24px

  .settings
    margin: 10px
    display: flex

  .setting
    display: flex
    flex-direction: column
    align-items: center
    font-size: 12px
    color: $bright1
    font-weight: bold
    white-space: nowrap
    & + .setting
      margin-left: 12px
    > :not(span)
      margin-top: 6px
    input
    .radio-bar
    .icon-toggle
      flex: 1 1 auto

  .confirmation
    display: flex
    flex-direction: column
    justify-content: space-around
    background: $dark1
    border-radius: 0 4px 4px 0

    button
      height: 100%
      background: none
      border: none
      color: $bright1
      font-size: 16px
      padding: 0px 9px 0px 11px
      transition: all 0.15s
      &:hover
        color: $cancel
      &:active
        color: darken($cancel, 15%)
        transition: none

</style>


<script>

  export default {
    name: 'ToolOptionsBox',

    inject: ['bus'],

    props: {
      activeTool: Object,
    },

    mounted: function() {
      this.bus.on('escape', this.close)
    },

    beforeUnmount: function() {
      this.bus.off('escape', this.close)
    },

    methods: {
      setSetting: function(key, value) {
        this.activeTool.setOption(key, value)
      },

      close: function() {
        this.$emit('close')
      },
    },
  }

</script>
