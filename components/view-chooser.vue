<template lang="pug">

  .view-chooser.bordered(@mouseleave="unhover")

    transition-group(
      name="list"
      tag="ul"
      v-if="document.views.length"
    )
      li(
        v-for="view in document.views"
        :key="view.id || view.title",
        :class="{ active: document.activeView == view }"
        @click="document.activateView(view)"
        @mouseenter="document.previewView = view"
      )
        .title {{ view.title }}

        //- Icon.view-settings(icon="ellipsis-v")

    transition(name="fade" mode="out-in")

      .controls(
        v-show="!!document.dirtyView"
        :class="{ settingsOpen, saveCamera: saveCamera && settingsOpen }"
        @mouseenter="unhover"
      )

        .buttons

          Icon.plus(icon="plus" @click="createView")

          Icon.create-settings(icon="ellipsis-v" @click="settingsOpen = !settingsOpen")

        .settings

          h1 Save State

          label
            input(type="checkbox" v-model="saveCamera")
            | Camera

          label
            input(type="checkbox" v-model="saveVisibility")
            | Part Visibility

          label
            input(type="checkbox" v-model="saveSections")
            | Section Views

          label
            input(type="checkbox" v-model="saveActiveComp")
            | Active Comp.

          label
            input(type="checkbox" v-model="saveMarker")
            | History Marker

          label
            input(type="checkbox" v-model="saveExpansion")
            | Tree Expansion

          .cam-settings

            h1 Camera Settings

            label
              input(type="checkbox" v-model="zoomToFit")
              | Zoom to fit

            label
              input(type="checkbox" v-model="rotationOnly")
              | Rotation only

</template>


<style lang="stylus" scoped>

  .view-chooser
    pointer-events: auto
    overflow: hidden
    display: flex
    flex-direction: column

  ul
    flex: 1 1 auto
    overflow: auto
    -webkit-overflow-scrolling: touch
    max-height: 210px
    -ms-overflow-style: none
    scrollbar-width: none

    &::-webkit-scrollbar
      display: none

    &:hover li
      transition: none

  li
    padding: 10px 16px
    font-size: 12px
    font-weight: bold
    text-align: center
    transition: all 0.25s

    &:hover
      background: $dark1

    &:active
      background: $dark1 * 0.9

    &.active
      background: $highlight
      color: white
      transition: all 0.25s !important

  .controls
    color: $bright1
    height: 28px
    border: none
    padding: 0
    margin-top: 1px
    border-top: 1px solid $dark1 * 1.1
    box-shadow: 0 0 6px rgba(black, 0.4)
    // text-shadow: 0 -1px 0px black
    transition: all 0.25s
    overflow: hidden
    flex: 0 0 auto
    font-size: 13px

    &:hover
      border-top-color: $dark1 * 1.25

    &:active
      transition: none

    &:only-child
      margin-top: 0
      border: none

    &.settingsOpen
      height: 198px

    &.saveCamera
      height: 266px

      .cam-settings
        opacity: 1

  .buttons
    display: flex
    align-items: center

  .plus
  .create-settings
    background: $dark2 * 1.1
    transition: all 0.25s
    padding: 7px 0.75rem

    &:hover
      background: $dark1
      transition: none

    &:active
      background: $dark2 * 0.8

  .plus
    flex: 1 1 auto

  .create-settings
    border-left: 1px solid $dark1 * 1.1
    background: $dark2 * 1.1

  .settings
    padding: 0.5rem
    padding-top: 0
    border-top: 1px solid $dark1 * 1.1
    background: $dark2 * 1.1

    h1
      font-size: 0.7rem
      font-weight: bold
      margin-block: 0.5rem

    label
      font-size: 0.7rem
      margin-top: 0.35rem
      white-space: nowrap

    input[type="checkbox"]
      margin-right: 0.35rem

  .cam-settings
    opacity: 0
    transition: all 0.25s 0.2s

  .fade-enter-from
  .fade-leave-to
    opacity: 0
    height: 0 !important
    padding: 0
    margin: 0

  .list-enter-active
  .list-leave-active
    transition: all 0.25s

  .list-enter-from
  .list-leave-to
    opacity: 0
    padding: 0
    margin: 0
    margin-bottom: -12px

</style>


<script setup>

  import { makeID } from './../js/core/id.js'

  const props = defineProps(['document'])

  const settingsOpen = ref(false)

  const saveCamera = ref(true)
  const saveVisibility = ref(true)
  const saveSections = ref(true)
  const saveActiveComp = ref(false)
  const saveMarker = ref(false)
  const saveExpansion = ref(false)

  const zoomToFit = ref(false)
  const rotationOnly = ref(false)

  function unhover() {
    props.document.previewView = props.document.dirtyView
  }

  function createView() {
    const allComps = props.document.top().getChildren()
    const view = {
      title: 'Custom ' + makeID().replace('id-', ''),
    }
    if(saveCamera.value) {
      view.position = props.document.dirtyView.position.clone()
      view.target = props.document.dirtyView.target.clone()
      view.zoomToFit = zoomToFit.value
      view.rotationOnly = rotationOnly.value
    }
    if(saveVisibility.value) {
      view.visibilities = {}
      allComps.forEach(comp => {
        view.visibilities[comp.id] = {
          compHidden: comp.creator.hidden,
          itemsHidden: JSON.parse(JSON.stringify(comp.creator.itemsHidden)),
        }
      })
    }
    if(saveActiveComp.value) view.activeComponentId = props.document.activeComponent.id
    if(saveMarker.value) view.marker = props.document.timeline.marker
    if(saveExpansion.value) {
      view.expansions = {}
      allComps.forEach(comp => {
        view.expansions[comp.id] = true
      })
    }
    props.document.addView(view)
  }

</script>
