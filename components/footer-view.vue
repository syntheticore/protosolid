<template lang="pug">
  footer.footer-view

    //- .tool-info.bordered
    //-   b Select Tool
    //-   Icon(icon="mouse" fixed-width)
    //-   | Select geometry
    //-   Icon(icon="mouse" fixed-width)
    //-   | Bring up actions

    transition(name="fade")
      .selection-info.bordered(v-if="toast || document.selection.items.length")

        template(v-if="toast")
          div
            span Rejected
            span.warn &nbsp;{{ toast }}

        template(v-else-if="document.selection.items.length")
          div
            span {{ description.title }}

          div(v-if="description.isMixed")
            span # Total
            span
              | {{ document.selection.items.length }}

          div(v-for="prop in description.properties")
            span {{ prop.title }}
            span(v-if="!prop.warn")
              | {{ prop.unit ? prop.value.toFixed(2) : prop.value }} {{ prop.unit }}
            span.warn(v-else) {{ prop.value }}

</template>


<style lang="stylus" scoped>

  .footer-view
    font-size: 13px
    padding: 12px
    color: $bright2
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
    display: flex
    align-items: center

    div
      padding: 2px 14px
      &:first-child
        padding: 1px 14px
        font-size: 14px
        span
          padding-right: 0
      &:not(:last-child)
        border-right: 1px solid $dark1

    span:first-child
      // text-align: right
      padding-right: 8px

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

  .warn
    color: $warn !important

  .debug-panel
    position: absolute
    right: calc(100vw / 2)
    bottom: 7px
    pointer-events: auto
    display: none
    white-space: nowrap
    [data-platform="browser"] &
      display: block

  .fade-enter-active
  .fade-leave-active
    transition: all 0.4s

  .fade-enter-from
  .fade-leave-to
    opacity: 0
    transform: translateY(-12px)

</style>


<script>

  import pluralize from 'pluralize'

  function groupBy(xs, key) {
    return xs.reduce(function(rv, x) {
      (rv[x[key]()] = rv[x[key]()] || []).push(x)
      return rv
    }, {})
  }

  export default {
    name: 'FooterView',

    inject: ['bus'],

    props: {
      document: Object,
    },

    data() {
      return{
        toast: null,
      }
    },

    mounted() {
      this.bus.on('toast', (msg) => {
        this.toast = msg
        setTimeout(() => this.toast = null, 5000)
      })
    },

    computed: {
      description: function() {
        const selection = [...this.document.selection.items]
        const uniqueNames = selection.map(item => item.typename() ).filter((value, index, self) => self.indexOf(value) === index )
        const groups = groupBy(selection, 'typename')
        const numGroups = Object.keys(groups).length
        const propsPerItem = selection.map(item => this.produceProperties(item) ).filter(Boolean)
        return {
          title: Object.keys(groups).map(typename => {
            const group = groups[typename]
            return `${group.length == 1 && numGroups == 1 ? '' : group.length} ${pluralize(typename, group.length)}`
          }).join(' + '),

          properties: propsPerItem.reduce((acc, propSet) => {
            Object.keys(acc).forEach(key => {
              const prop = acc[key]
              const otherProp = propSet[key]
              if(!otherProp) {
                delete acc[key]
              } else if(typeof prop.value == 'number' && typeof otherProp.value == 'number') {
                prop.value += otherProp.value
              }
              prop.title = key
            })
            return acc
          }),

          isMixed: numGroups > 1,
        }
      },
    },

    methods: {
      produceProperties: function(item) {
        const type = item.typename().split(' ')[0]
        if(type == 'Solid') {
          const volume = item.volume()
          const weight = item.component.creator.material ? volume * item.component.creator.material.density / 1000 : 'No Material'
          return {
            Weight: {
              title: 'Weight',
              unit: 'g',
              value: weight,
              warn: !item.component.creator.material,
            },
            Volume: {
              title: 'Volume',
              unit: 'cm³',
              value: volume / 1000.0,
            },
            Area: {
              title: 'Surface Area',
              unit: 'cm²',
              value: item.area() / 100.0,
            },
          }
        } else if(type == 'Component') {
          const weight = item.getWeight() / 1000
          return {
            Weight: {
              title: 'Weight',
              unit: 'g',
              value: weight ? weight : 'No Material',
              warn: !weight,
            },
            Volume: {
              title: 'Volume',
              unit: 'cm³',
              value: item.compound.volume() / 1000.0,
            },
            ...item.children.length && { TotalVolume: {
              title: 'Total Volume',
              unit: 'cm³',
              value: item.getVolume() / 1000.0,
            } },
            Area: {
              title: 'Surface Area',
              unit: 'cm²',
              value: item.compound.area() / 100.0,
            },
          }
        } else if(type == 'Line' || type == 'Spline') {
          return {
            Length: {
              title: 'Length',
              unit: 'mm',
              value: item.length(),
            },
          }
        } else if(type == 'Circle') {
          return {
            Radius: {
              title: 'Radius',
              unit: 'mm',
              value: item.radius
            },
            Diameter: {
              title: 'Diameter',
              unit: 'mm',
              value: item.radius * 2
            },
            Length: {
              title: 'Circumfence',
              unit: 'mm',
              value: item.length()
            },
            Area: {
              title: 'Area',
              unit: 'cm²',
              value: item.area() / 100.0
            },
          }
        } else if(type == 'Arc') {
          return {
            Radius: {
              title: 'Radius',
              unit: 'mm',
              value: item.radius
            },
            Length: {
              title: 'Length',
              unit: 'mm',
              value: item.length()
            },
          }
        } else if(type == 'Sketch') {
          return {
            '# Elements': {
              title: '# Elements',
              value: (item.sketch || item).elements.length,
            },
          }
        } else {
          return {}
        }
      },
    },
  }
</script>
