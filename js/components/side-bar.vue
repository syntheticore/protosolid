<template lang="pug">
  .side-bar
    slot
</template>


<style lang="stylus" scoped>
  .side-bar
    // padding-top: 38px
    padding-top: 16px
    padding-right: 16px
    overflow: hidden
  h1
    color: $bright2
    color: gray * 1.5
    font-size: 0.8rem
    font-weight: bold
    letter-spacing: 1px
    margin-left: 12px
    margin-bottom: 6px
    text-transform: uppercase
    text-shadow: 0 1px 4px rgba(0,0,0, 0.3)
</style>


<script>
  // import TreeView from './tree-view.vue'

  export default {
    name: 'SideBar',
    components: {
      // TreeView,
    },
    data() {
      return {
        // views: {

        // },
        // poses: {

        // },
        // tree: {
        //   id: '1',
        //   name: 'Main Assembly',
        //   children: [
        //     {
        //       id: '2',
        //       name: 'Part 1',
        //     },
        //     {
        //       id: '3',
        //       name: 'Sub Assembly 1',
        //       children: [
        //         {
        //           id: '4',
        //           name: 'Part 2',
        //         },
        //         {
        //           id: '5',
        //           name: 'Part 3',
        //         },
        //         {
        //           id: '6',
        //           name: 'Sub Assembly 2',
        //           children: [
        //             {
        //               id: '7',
        //               name: 'Part 4',
        //             },
        //             {
        //               id: '8',
        //               name: 'Part 5',
        //             },
        //           ]
        //         },
        //         {
        //           id: '9',
        //           name: 'Sub Assembly 2',
        //           children: [
        //             {
        //               id: '10',
        //               name: 'Part 6',
        //             },
        //             {
        //               id: '11',
        //               name: 'Part 7',
        //             },
        //             {
        //               id: '12',
        //               name: 'Part 8',
        //             },
        //           ]
        //         },
        //       ]
        //     },
        //   ]
        // }
      }
    }
  }
</script>
