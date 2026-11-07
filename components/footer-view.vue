<template lang="pug">
  footer.footer-view

    //- .tool-info.bordered
    //-   b Select Tool
    //-   Icon(icon="mouse" fixed-width)
    //-   | Select geometry
    //-   Icon(icon="mouse" fixed-width)
    //-   | Bring up actions

    transition(name="fade")
      .selection-info.bordered(v-pixel-snap v-if="calculation || toast || document.selection.items.length")

        template(v-if="calculation")
          div
            span {{ calculation.title }}
            span &nbsp;{{ calculation.detail }}
          div(v-if="calculation.current && calculation.total")
            span Feature
            span {{ calculation.current }} / {{ calculation.total }}

        template(v-else-if="toast")
          div
            span {{ toast.title }}
            span.warn &nbsp;{{ toast.message }}

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
              | {{ prop.approximate ? '≈ ' : '' }}{{ prop.unit ? prop.value.toFixed(2) : prop.value }} {{ prop.unit }}
            span.warn(v-else) {{ prop.value }}

</template>


<style lang="stylus" scoped>

  .footer-view
    font-size: 13px
    padding: 12px
    color: bright2()
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
        border-right: 1px solid dark1()

    span:first-child
      // text-align: right
      padding-right: 8px

    span:last-child
      color: bright1()
      font-weight: bold

  b
    margin-right: 6px
    color: bright1(0.9)

  svg
    margin-left: 9px
    margin-right: 3px
    color: bright1()

  .warn
    color: var(--warn) !important

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
  import { shapex as sx } from '../js/core/shapex.js'
  import { selectionType } from '../js/core/geom3d.js'

  function groupBy(xs, key) {
    return xs.reduce(function(rv, x) {
      const name = key(x)
      rv[name] ||= []
      rv[name].push(x)
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
        calculation: null,
      }
    },

    mounted() {
      this.bus.on('calculation', (calculation) => {
        this.calculation = calculation
      })

      this.bus.on('calculation-finished', (id) => {
        if(this.calculation?.id == id) this.calculation = null
      })
      
      this.bus.on('toast', (msg) => {
        this.toast = typeof msg == 'string' ? { title: 'Rejected', message: msg } : msg
        setTimeout(() => this.toast = null, 5000)
      })
    },

    computed: {
      description: function() {
        const selection = [...this.document.selection.items]
        const uniqueNames = selection.map(item => item.typename() ).filter((value, index, self) => self.indexOf(value) === index )
        const groups = groupBy(selection, selectionType)
        const numGroups = Object.keys(groups).length
        const propsPerItem = selection.map(item => this.produceProperties(item) ).filter(Boolean)
        const topologySelection = uniqueNames.every(type => type == 'Face' || type == 'Edge')
        return {
          title: Object.keys(groups).map(typename => {
            const group = groups[typename]
            return `${group.length == 1 && numGroups == 1 ? '' : group.length} ${pluralize(typename, group.length)}`
          }).join(' + '),

          properties: propsPerItem.reduce((acc, propSet) => {
            if(topologySelection) Object.keys(propSet).forEach(key => {
              if(!acc[key]) acc[key] = { ...propSet[key], value: 0 }
            })
            Object.keys(acc).forEach(key => {
              const prop = acc[key]
              const otherProp = propSet[key]
              if(!otherProp) {
                if(!topologySelection) delete acc[key]
              } else if(typeof prop.value == 'number' && typeof otherProp.value == 'number') {
                prop.value += otherProp.value
                prop.approximate ||= otherProp.approximate
              }
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
          const material = item.component.getMaterial()
          const weight = material ? volume * material.density / 1000 : 'No Material'
          return {
            Weight: {
              title: 'Weight',
              unit: 'g',
              value: weight,
              warn: !material,
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
        } else if(item.typename() == 'Component') {
          const weight = item.getWeight()
          const hasWeight = weight !== undefined
          return {
            Weight: {
              title: 'Weight',
              unit: 'g',
              value: hasWeight ? weight / 1000 : 'No Material',
              warn: !hasWeight,
            },
            Volume: {
              title: 'Volume',
              unit: 'cm³',
              value: item.getVolume() / 1000.0,
            },
            Area: {
              title: 'Surface Area',
              unit: 'cm²',
              value: item.getArea() / 100.0,
            },
          }
        } else if(type == 'Face') {
          return {
            Area: {
              title: 'Surface Area',
              unit: 'cm²',
              value: item.area() / 100.0,
            },
          }
        } else if(type == 'Edge') {
          const curve = item.geom().curve()
          const circular = curve instanceof sx.Circle3
          const properties = {
            Length: {
              title: 'Length',
              unit: 'mm',
              value: item.length(),
              approximate: !(curve instanceof sx.Line3 || circular),
            },
          }
          if(circular && this.document.selection.items.length == 1) {
            properties.Radius = { title: 'Radius', unit: 'mm', value: curve.radius() }
            properties.Diameter = { title: 'Diameter', unit: 'mm', value: curve.radius() * 2 }
          }
          return properties
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
