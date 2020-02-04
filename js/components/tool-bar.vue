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
    color: $bright1
    background: $dark2
    background: linear-gradient(top, $dark1, $dark2)
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
      // border-radius: 4px 4px 0 0
      // border-top: 1px solid transparent
      padding: 12px 12px
      // margin-top: 7px
      // margin-right: 12px
      height: 36px
      font-size: 12px
      font-weight: bold
      // text-align: right
      min-width: 160px
      // color: $bright1
      // background: $dark1
      // background: linear-gradient(top, $dark1 * 1.2, $dark2)
      transition: all 0.2s
      cursor: pointer
      &:first-child
        // border-left: 1px solid blue
      &:hover
        background: $dark1
      &.active
        background: $dark1 * 1.2
        // background: #316791
        // border-top: 1px solid $highlight
        border-left: 1px solid $dark1 * 1.4
        // border-left: 1px solid #316791 * 1.15
        // box-shadow: 0 0 6px rgba(0,0,0, 0.4)
      & + li
        border-left: 1px solid $dark1
        
      svg
        margin-left: 8px
        color: $bright2
        cursor: pointer
        transition: all 0.2s
        float: right
        &:hover
          color: $bright1
  .menu-btn
    background: $dark1
    box-shadow: 0 0 0 1px $dark1 * 0.5
    border: 1px solid $dark1 * 1.1
    border-top-color: $dark1 * 1.6
    background: linear-gradient(top, $dark1 * 1.2, $dark2 * 1.3)
    border-radius: 4px
    font-size: 16px
    color: $bright1
    padding: 4px 8px
    margin: 4px
    .expander
      margin-left: 8px
      color: $bright2
  .grab-handle
    -webkit-app-region: drag
    -webkit-user-select: none
    flex: 1 1 auto
  .account
    padding: 4px 4px 22px 14px
    border-radius: 99px
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
  // import TreeItem from './tree-item.vue'

  export default {
    name: 'ToolBar',
    components: {

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
