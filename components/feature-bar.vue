<template lang="pug">
  footer.feature-bar.bordered

    nav.controls
      button(@click="rewindMarker", :disabled="atStart")
        Icon(icon="angle-double-left")

      button(@click="stepMarker(-1)", :disabled="atStart")
        Icon(icon="angle-left")

      button(@click="stepMarker(1)", :disabled="atEnd")
        Icon(icon="angle-right")

      button(@click="forwardMarker", :disabled="atEnd")
        Icon(icon="angle-double-right")

    ul.features(ref="features", @scroll="onScroll", :style="{'--tip': tip + 'px'}")
      li.past(
        v-for="(feature, i) in past",
        :ref="feature == document.activeFeature ? 'active' : undefined"
        :style="{'--color': getFeatureColor(feature)}"
      )
        transition(name="fade")
          FeatureBox.tipped-bottom(ref="box"
            v-if="isActive(feature)"
            show-header
            :style="scrollStyle"
            :class="{ bright: tip < 55 }"
            :document="document"
            :active-tool="activeTool"
            :active-feature="document.activeFeature"
            @close="document.activateFeature(null, true, true)"
          )
          //- @close="$emit('update:active-feature', null, true)"
        FeatureIcon(
          :document="document"
          :feature="feature"
          :is-active="isActive(feature)"
          @move-marker="document.moveMarker(i + 1)"
        )
        //- v-on="$listeners"
        //- :selection="document.selection"
      hr(ref="marker")
      li.future(
        v-for="(feature, i) in future"
        :style="{'--color': getFeatureColor(feature)}"
      )
        FeatureIcon(
          isFuture
          :document="document"
          :feature="feature"
          :is-active="isActive(feature)"
          @move-marker="document.moveMarker(past.length + i + 1)"
        )
        //- v-on="$listeners"
        //- :selection="document.selection"
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
      background: linear-gradient(to right, $dark2 * 0.8, rgba($dark2, 0))
      z-index: 1
    &::after
      left: unset
      right: 0
      background: linear-gradient(to left, $dark2, rgba($dark2, 0))
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

  .fade-enter-from, .fade-leave-to
    opacity: 0
    transform: translateY(-10px)
</style>


<script>

  import { inject } from 'vue'

  // import FeatureBox from './feature-box.vue'
  // import FeatureIcon from './feature-icon.vue'

  export default {
    name: 'FeatureBar',

    inject: ['bus'],

    // components: {
    //   FeatureBox,
    //   FeatureIcon,
    // },

    props: {
      document: Object,
      activeTool: Object,
      // activeComponent: Object,
      // activeFeature: Object,
      // selection: Object,
    },

    computed: {
      past: function() {
        const past = this.document.timeline.features.slice(0, this.document.timeline.marker)
        return past.filter(feature => this.shouldDisplayFeature(feature) )
      },

      future: function() {
        const future = this.document.timeline.features.slice(this.document.timeline.marker)
        return future.filter(feature => this.shouldDisplayFeature(feature) )
      },

      atStart: function() {
        return this.document.timeline.marker == 0
      },

      atEnd: function() {
        return this.document.timeline.marker == this.document.timeline.features.length
      },

      scrollStyle: function() {
        return {
          left: String(this.scrolled + 145) + 'px',
        }
      },

      relevantComponentIds() {
        return this.document.timeline.getFutureChildIds(this.document.activeComponent.id)
      },
    },

    data() {
      return {
        scrolled: 0,
        tip: 0,
      }
    },

    watch: {
      'document.activeFeature': function(feature) {
        if(!feature) return
        setTimeout(() => {
          if(!this.isElemVisible(this.$refs.active[0])) this.scrollToElem(this.$refs.active[0])
          this.onScroll()
        })
      },

      'document.timeline.marker': function() {
        // this.$emit('update:active-feature', null)
        setTimeout(() => {
          if(!this.isElemVisible(this.$refs.marker)) this.scrollToElem(this.$refs.marker)
        })
      },
    },

    mounted() {
      this.bus.on('resize', this.onScroll)
    },

    methods: {
      isActive(feature) {
        return this.document.activeFeature && (feature == this.document.activeFeature)
      },

      // moveMarker: function(i) {
      //   this.document.activateFeature(null, true, false)
      //   this.document.timeline.marker = i
      //   this.document.regenerate()
      // },

      rewindMarker: function() {
        this.document.moveMarker(0)
      },

      stepMarker: function(incr) {
        this.document.moveMarker(Math.min(
          this.document.timeline.features.length,
          Math.max(0, this.document.timeline.marker + incr)
        ))
      },

      forwardMarker: function() {
        this.document.moveMarker(this.document.timeline.features.length)
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
        const compId = feature.modifiedComponents()[0]
        return compId && this.document.timeline.finalTree().findChild(compId).creator.color //XXX super slow
      },

      shouldDisplayFeature(feature) {
        const modified = feature.modifiedComponents()
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
