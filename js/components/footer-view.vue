<template lang="pug">
  footer.footer-view

    //- .tool-info.bordered
    //-   b Select Tool
    //-   fa-icon(icon="mouse" fixed-width)
    //-   | Select geometry
    //-   fa-icon(icon="mouse" fixed-width)
    //-   | Bring up actions

    transition(name="fade")
      .selection-info.bordered(v-if="selectedElement")
        div
          span # Objects
          span 1 {{type}}

        div(v-if="type == 'Solid'")
          span Weight
          span 12.3 g

        div(v-if="type == 'Solid'")
          span Volume
          span 140 cm³

        div(v-if="type == 'Solid'")
          span Surface Area
          span 54 cm³

        div(v-if="type == 'Line' || type == 'BezierSpline'")
          span Length
          span {{selectedElement.get_length().toFixed(2)}} mm

        div(v-if="type == 'Circle'")
          span Radius
          span {{selectedElement.get_radius().toFixed(2)}} mm

        div(v-if="type == 'Circle'")
          span Diameter
          span {{(selectedElement.get_radius() * 2).toFixed(2)}} mm

        div(v-if="type == 'Circle'")
          span Perimeter
          span {{selectedElement.get_length().toFixed(2)}} mm

        div(v-if="type == 'Circle'")
          span Area
          span {{selectedElement.get_area().toFixed(2)}} mm²

    .debug-panel
      button.button(@click="splitAll") Split all
      button.button(@click="exportStl") Export STL
</template>


<style lang="stylus" scoped>
  .footer-view
    font-size: 13px
    padding: 12px
    color: $bright2
    text-shadow: 0 1px 3px black
    display: flex
    // justify-content: space-between
    justify-content: flex-end
    align-items: flex-end
    pointer-events: none

  .tool-info
    // flex: 0 0 content
    padding: 16px 20px
    border-radius: 99px

  .selection-info
    line-height: 1.9
    padding: 4px 14px
    // table-layout: fixed
    white-space: nowrap
    padding: 0
    align-self: flex-end

    div
      display: inline-block
      padding: 2px 14px
      &:not(:last-child)
        border-right: 1px solid $dark1

    span:first-child
      // text-align: right
      padding-right: 4px

    span:last-child
      color: $bright1
      font-weight: bold

  b
    margin-right: 6px
    color: $bright1 * 0.9

  svg
    margin-left: 9px
    margin-right: 3px
    color: $bright1

  .debug-panel
    position: absolute
    left: 5px
    bottom: 7px
    pointer-events: all

  .fade-enter-active, .fade-leave-active
    transition: all 0.4s

  .fade-enter, .fade-leave-to
    opacity: 0
    transform: translateY(12px)
</style>


<script>
  // import Foo from './foo.vue'

  export default {
    name: 'FooterView',

    components: {},

    props: {
      selectedElement: Object,
      activeComponent: Object,
    },

    computed: {
      type: function() {
        return this.selectedElement.typename()
      },
    },

    methods: {
      splitAll: function() {
        // const regions = this.activeComponent.get_regions()
        // console.log('Regions', regions)
        const splits = this.activeComponent.get_sketch().get_all_split()
        // const elems = this.activeComponent.get_sketch_elements()
        // // .map(elem => elem.get_handles())
        // elems.forEach(elem => {
        //   this.activeComponent.remove_element(elem.id())
        // })
        document._debug.viewport.componentChanged(this.activeComponent)
      },

      saveFile: function(data, filename, filetype) {
        var file = new Blob([data], {filetype});
        const a = document.createElement("a")
        const url = URL.createObjectURL(file);
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        setTimeout(function() {
          document.body.removeChild(a);
          window.URL.revokeObjectURL(url);
        }, 0);
      },

      exportStl: function() {
        const stl = this.activeComponent.export_stl()
        const title = this.activeComponent.get_title().replace(' ', '_') + '.stl'
        this.saveFile(stl, title, 'STL')
      },
    },
  }
</script>
