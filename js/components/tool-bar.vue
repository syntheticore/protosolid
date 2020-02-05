<template lang="pug">
  header.tool-bar
    ul.tabs
      li(v-for="doc in documents" :class="{active: doc.active}")
        span.title {{doc.title}}
        fa-icon(icon="times")
    .grab-handle
    nav
      button.menu-btn
        fa-icon(icon="cloud")
        fa-icon.expander(icon="angle-down")
      PopUp
      button.menu-btn
        fa-icon(icon="pen")
        fa-icon.expander(icon="angle-down")
      button.menu-btn
        fa-icon(icon="ruler")
        fa-icon.expander(icon="angle-down")
    button.menu-btn.account
      span.name Björn
      fa-icon(icon="user-circle")
</template>


<style lang="stylus" scoped>
  .tool-bar
    display: flex
    overflow: hidden
    padding-left: 84px
    background: $dark2
    background: linear-gradient(top, $dark1, $dark2 * 0.95)
    border-top: 1px solid $dark1 * 1.3
    border-bottom: 1px solid black
    border-radius: 5px 5px 0px 0px
    border-radius-bottom: 0
  nav
    margin-right: 6px
  .tabs
    display: inline-block
    flex: 1 1 content
    li
      display: inline-block
      border-left: 1px solid $dark1 * 1.2
      border-right: 1px solid black //#353535
      // box-shadow: 0 0 3px black
      border-bottom: none
      padding: 12px 12px
      height: 36px
      font-size: 12px
      font-weight: bold
      min-width: 160px
      transition: all 0.2s
      color: $bright2
      text-shadow: 0 -1px 0px black
      // cursor: pointer
      &:hover
        background: $dark1
      &.active
        color: $bright1
        background: $dark1 * 1.2
        // border-top: 1px solid $highlight
        border-left: 1px solid $dark1 * 1.4
      & + li
        border-left: 1px solid $dark1        
      svg
        margin-left: 8px
        color: $bright2
        cursor: pointer
        transition: all 0.2s
        float: right
        filter: drop-shadow(0 1px 0px rgba(0,0,0, 0.9))
        &:hover
          color: $bright1
  .menu-btn
    // background: $dark1
    box-shadow: 0 0 0 1px $dark2 * 0.55
    border: 1px solid $dark1 * 1.1
    border-top-color: $dark1 * 1.6
    background: linear-gradient(top, $dark1 * 1.2, $dark2 * 1.3)
    border-radius: 4px
    font-size: 16px
    color: $bright1
    padding: 4px 8px
    margin: 4px
    // cursor: pointer
    transition: background 0.2s
    &:hover
      background: linear-gradient(top, $dark1 * 1.3, $dark2 * 1.4)
      border-top-color: $dark1 * 1.7
    &:active
      border: 1px solid $dark1 * 0.95
      background: linear-gradient(top, $dark1 * 1.1, $dark2 * 1.275)
    .expander
      margin-left: 8px
      color: $bright2
    svg
      filter: drop-shadow(0 -1px 0px rgba(0,0,0, 0.6))
  .grab-handle
    -webkit-app-region: drag
    -webkit-user-select: none
    flex: 1 1 auto
  .account
    padding: 4px 4px 22px 14px
    margin: 4px 5px
    // border-radius: 99px
    font-size: 12px
    font-weight: bold
    flex: 0 0 auto
    .name
      position: relative
      top: -3px
    svg
      font-size: 18px
      margin-left: 6px
</style>


<script>
  import PopUp from './pop-up.vue'

  export default {
    name: 'ToolBar',
    components: {
      PopUp,
    },
    
    // props: {
    //   documents: Array,
    // },

    data() {
      return {
        documents: [
          { title: 'Steam Engine', active: false },
          { title: 'Print Head', active: true },
          { title: 'Tool Holder' },
        ]
      };
    }
  }
</script>
