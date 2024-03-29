<template lang="pug">
  footer.feature-bar.bordered

    nav.controls
      button(@click="rewindMarker", :disabled="atStart")
        fa-icon(icon="angle-double-left")

      button(@click="stepMarker(-1)", :disabled="atStart")
        fa-icon(icon="angle-left")

      button(@click="stepMarker(1)", :disabled="atEnd")
        fa-icon(icon="angle-right")

      button(@click="forwardMarker", :disabled="atEnd")
        fa-icon(icon="angle-double-right")

    ul.features(ref="features", @scroll="onScroll", :style="{'--tip': tip + 'px'}")
      li.past(
        v-for="(feature, i) in past",
        :ref="feature == activeFeature ? 'active' : undefined"
        :style="{'--color': getFeatureColor(feature)}"
      )
        transition(name="fade")
          FeatureBox.tipped-bottom(ref="box"
            v-if="isActive(feature)"
            show-header
            :style="scrollStyle"
            :class="{bright: tip < 55}"
            :document="document"
            :active-tool="activeTool"
            :active-feature="activeFeature"
            @close="$emit('update:active-feature', null, true)"
          )
        FeatureIcon(
          :feature="feature"
          :selection="selection"
          :is-active="isActive(feature)"
          @move-marker="setMarker(i + 1)"
          v-on="$listeners"
        )
      hr(ref="marker")
      li.future(
        v-for="(feature, i) in future"
        :style="{'--color': getFeatureColor(feature)}"
      )
        FeatureIcon(
          isFuture
          :feature="feature"
          :selection="selection"
          :is-active="isActive(feature)"
          @move-marker="setMarker(past.length + i + 1)"
          v-on="$listeners"
        )
</template>


<style lang="stylus" scoped>
  .feature-bar
    font-size: 13px
    color: $bright2
    display: flex
    // align-items: center

  .controls
    border-right: 1px solid $dark1
    flex: 1 0 auto
    position: relative
    button
      background: none
      border: none
      color: $bright2
      padding: 20px 15px
      transition: all 0.15s
      &:first-child
        border-top-left-radius: 4px
        border-bottom-left-radius: 4px
      &:hover
        background: $dark1
        transition: none
      &:active
        background: $dark2 * 0.85
      &:disabled
        opacity: 0.3

  .features
    display: flex
    overflow-x: auto
    overflow-y: hidden
    scroll-behavior: smooth
    padding: 0 6px
    border-radius: 15px
    align-items: center
    scrollbar-color: $dark1 * 1.15 $dark2
    scrollbar-width: thin
    &::-webkit-scrollbar
      height: 8px
      background-color: $dark2
      border-bottom-right-radius: 4px
    &::-webkit-scrollbar-thumb
      background: $dark1 * 1.15
      // border-radius: 4px
      &:hover
        background: $dark1 * 1.3
    &::before
    &::after
      position: absolute
      content: ''
      left: 157px
      width: 12px
      top: 0
      height: 100%
      background: linear-gradient(left, $dark2 * 0.8, rgba($dark2, 0))
      z-index: 1
    &::after
      left: unset
      right: 0
      background: linear-gradient(right, $dark2, rgba($dark2, 0))
      border-top-right-radius: 4px
      border-bottom-right-radius: 4px
    > hr
      border: none
      width: 5px
      height: 5px
      background: $highlight
      border-radius: 10px
      margin: 7px
      flex: 0 0 auto
      &:first-child
        margin-left: 14px
        margin-right: 0px
      &:last-child
        margin-left: 0px
        margin-right: 14px
      &:only-child
        margin: 7px
    > li
      border-bottom: 1px solid var(--color)
      &:first-of-type
        margin-left: 14px
      &:last-of-type
        margin-right: 11px

  .feature-box
    bottom: 0
    margin-bottom: 61px
    position: absolute
    &::before
      left: var(--tip)

  .fade-enter-active, .fade-leave-active
    transition: opacity 0.15s ease-out, transform 0.15s ease-out

  .fade-enter, .fade-leave-to
    opacity: 0
    transform: translateY(-10px)
</style>


<script>
  import FeatureBox from './feature-box.vue'
  import FeatureIcon from './feature-icon.vue'

  export default {
    name: 'FeatureBar',

    components: {
      FeatureBox,
      FeatureIcon,
    },

    props: {
      document: Object,
      activeTool: Object,
      activeComponent: Object,
      activeFeature: Object,
      selection: Object,
    },

    computed: {
      past: function() {
        const past = this.document.features.slice(0, this.marker)
        return past.filter(feature => this.shouldDisplayFeature(feature) )
      },

      future: function() {
        const future = this.document.features.slice(this.marker)
        return future.filter(feature => this.shouldDisplayFeature(feature) )
      },

      atStart: function() {
        return this.marker == 0
      },

      atEnd: function() {
        return this.marker == this.document.features.length
      },

      scrollStyle: function() {
        return {
          left: String(this.scrolled + 145) + 'px',
        }
      },

      relevantComponentIds() {
        return this.document.getFutureChildIds(this.activeComponent.id)
      },
    },

    data() {
      return {
        marker: 0,
        scrolled: 0,
        tip: 0,
      }
    },

    watch: {
      document: function() {
        this.updateMarker()
      },

      activeFeature: function(feature) {
        if(!feature) return
        setTimeout(() => {
          if(!this.isElemVisible(this.$refs.active[0])) this.scrollToElem(this.$refs.active[0])
          this.onScroll()
        })
      },

      marker: function() {
        this.$emit('update:active-feature', null)
        setTimeout(() => {
          if(!this.isElemVisible(this.$refs.marker)) this.scrollToElem(this.$refs.marker)
        })
      },
    },

    mounted() {
      this.$root.$on('regenerate', this.updateMarker)
      this.$root.$on('resize', this.onScroll)
    },

    methods: {
      updateMarker() {
        this.marker = this.document.real.marker
      },

      isActive(feature) {
        return this.activeFeature && (feature.id == this.activeFeature.id)
      },

      setMarker(index) {
        this.document.real.marker = index
        this.$root.$emit('regenerate')
      },

      rewindMarker: function() {
        this.document.real.marker = 0
        this.$root.$emit('regenerate')
      },

      stepMarker: function(incr) {
        this.document.real.marker = Math.min(
          this.document.features.length,
          Math.max(0, this.document.real.marker + incr)
        )
        this.$root.$emit('regenerate')
      },

      forwardMarker: function() {
        this.document.real.marker = this.document.features.length
        this.$root.$emit('regenerate')
      },

      isElemVisible(elem) {
        const { left, right, width } = elem.getBoundingClientRect()
        const container = this.$refs.features.getBoundingClientRect()
        return left <= container.left ? container.left - left <= width : right - container.right <= width
      },

      scrollToElem(elem) {
        this.$refs.features.scrollLeft = elem.offsetLeft - (this.$refs.features.clientWidth / 2)
      },

      getFeatureColor(feature) {
        const compId = feature.real.modified_components()[0]
        return compId && this.document.componentData()[compId].color
      },

      shouldDisplayFeature(feature) {
        const modified = feature.real.modified_components()
        return modified.length == 0 || modified.some(compId =>
          this.relevantComponentIds.some(childId => childId === compId )
        )
      },

      onScroll: function() {
        if(!(this.$refs.active && this.$refs.active[0])) return
        this.$refs.box[0].updatePaths()
        const featuresLeft = this.$refs.features.getBoundingClientRect().left
        const boxWidth = this.$refs.box[0].$el.getBoundingClientRect().width
        const iconLeft = this.$refs.active[0].getBoundingClientRect().left
        const scrolled = iconLeft - featuresLeft
        const max = document.body.clientWidth - featuresLeft - boxWidth
        this.scrolled = Math.max(-146, Math.min(scrolled, max))
        this.tip = Math.max(24, Math.min(scrolled - this.scrolled + 24, boxWidth - 60))
      },
    },
  }
</script>
