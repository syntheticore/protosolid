<template lang="pug">
  .side-bar
    h1 Tree
    TreeView(:top="tree")
    //- h1 Views
    //- TreeView(:top="views")
    //- h1 Poses
    //- TreeView(:top="poses")
</template>


<style lang="stylus" scoped>
  .side-bar
    // padding-top: 38px
    padding-top: 16px
    padding-right: 16px
    overflow: hidden
    // background: linear-gradient(
    //   to right,
    //   hsla(0, 0%, 0%, 0.31) 0%,
    //   hsla(0, 0%, 0%, 0.306) 8.1%,
    //   hsla(0, 0%, 0%, 0.295) 15.5%,
    //   hsla(0, 0%, 0%, 0.278) 22.5%,
    //   hsla(0, 0%, 0%, 0.256) 29%,
    //   hsla(0, 0%, 0%, 0.23) 35.3%,
    //   hsla(0, 0%, 0%, 0.201) 41.2%,
    //   hsla(0, 0%, 0%, 0.17) 47.1%,
    //   hsla(0, 0%, 0%, 0.14) 52.9%,
    //   hsla(0, 0%, 0%, 0.109) 58.8%,
    //   hsla(0, 0%, 0%, 0.08) 64.7%,
    //   hsla(0, 0%, 0%, 0.054) 71%,
    //   hsla(0, 0%, 0%, 0.032) 77.5%,
    //   hsla(0, 0%, 0%, 0.015) 84.5%,
    //   hsla(0, 0%, 0%, 0.004) 91.9%,
    //   hsla(0, 0%, 0%, 0) 100%
    // )
    // background: linear-gradient(left, transparentify(black, 0.2), transparent)
    // background: $dark2
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
  .tree-view
    margin-bottom: 22px
</style>


<script>
  import TreeView from './tree-view.vue'

  export default {
    name: 'SideBar',
    components: {
      TreeView,
    },
    data() {
      return {
        views: {

        },
        poses: {

        },
        tree: {
          id: '1',
          name: 'Main Assembly',
          children: [
            {
              id: '2',
              name: 'Part 1',
            },
            {
              id: '3',
              name: 'Sub Assembly 1',
              children: [
                {
                  id: '4',
                  name: 'Part 2',
                },
                {
                  id: '5',
                  name: 'Part 3',
                },
                {
                  id: '6',
                  name: 'Sub Assembly 2',
                  children: [
                    {
                      id: '7',
                      name: 'Part 4',
                    },
                    {
                      id: '8',
                      name: 'Part 5',
                    },
                  ]
                },
                {
                  id: '9',
                  name: 'Sub Assembly 2',
                  children: [
                    {
                      id: '10',
                      name: 'Part 6',
                    },
                    {
                      id: '11',
                      name: 'Part 7',
                    },
                    {
                      id: '12',
                      name: 'Part 8',
                    },
                  ]
                },
              ]
            },
          ]
        }
      }
    }
  }
</script>
