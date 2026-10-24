<template lang="pug">

  .merge-panel.bordered(v-pixel-snap @keydown.stop)

    h1
      Icon(icon="compress-alt" fixed-width)
      span Merge
      span.branch(:style="{ '--color': colors[merge.branch] }") {{ merge.branch }}
      span into
      span.branch.current(:style="{ '--color': colors[merge.current] }") {{ merge.current }}

    input.input.large(
      v-if="merge.plan.type == 'merge'"
      type="text"
      placeholder="Describe this merge (optional)"
      v-model="merge.message"
      @keydown.enter="commit"
    )

    section(v-if="merge.changes.length || otherConflicts.length")

      h2 Changes
        span.count {{ merge.changes.length }}
        span.conflict-count(v-if="merge.conflicts.length") {{ merge.conflicts.length }} conflicting

      //- Merged branch left, current branch right, like the branch order in the header
      .changes(:class="{ conflicting: merge.conflicts.length }")

        RadioBar.keep-all(
          v-if="merge.conflicts.length"
          label="Keep for all conflicts"
          :items="sideItems(side => side.branch, side => `Keep all conflicting values from '${side.branch}'`)"
          :chosen="sideEverywhere"
          @update:chosen="resolve(merge.conflicts, $event)"
        )

        template(v-for="change in merge.changes" :key="change.id")

          .feature(
            :class="change.status"
            @mouseenter="highlightFeature(change.id)"
            @mouseleave="highlight = null"
          )
            Icon(:icon="change.icon" fixed-width)
            span.name {{ change.name }}
            span.status(v-if="change.status") {{ change.status }}

          template(v-for="conflict in conflictsByFeature[change.id]" :key="conflict.key")
            .label.nested(
              :title="conflict.label"
              @mouseenter="highlightFeature(change.id)"
              @mouseleave="highlight = null"
            ) {{ conflict.label }}
            RadioBar(
              :label="conflict.label"
              :items="sideItems(side => conflict[side.key], side => `${conflict[side.key]} (${side.branch})`)"
              :chosen="merge.resolutions[conflict.key]"
              @update:chosen="resolve([conflict], $event)"
            )

        template(v-for="conflict in otherConflicts" :key="conflict.key")
          .label(:title="conflict.label")
            Icon(:icon="conflict.icon" fixed-width)
            span {{ conflict.label }}
          RadioBar(
            :label="conflict.label"
            :items="sideItems(side => conflict[side.key], side => `${conflict[side.key]} (${side.branch})`)"
            :chosen="merge.resolutions[conflict.key]"
            @update:chosen="resolve([conflict], $event)"
          )

    p.note(v-else) No features change.

    .actions
      button.button.large(type="button" :disabled="busy" @click="merge.cancel()") Cancel
      button.button.large.primary(
        type="button"
        :disabled="busy"
        @click="commit"
      ) {{ merge.plan.type == 'merge' ? 'Merge' : 'Fast-forward' }}

</template>


<style lang="stylus" scoped>

  panelPadding = 13px

  .merge-panel
    width: 460px
    max-width: calc(100vw - 340px)
    max-height: calc(100vh - 230px)
    display: flex
    flex-direction: column
    gap: 10px
    padding: panelPadding
    border-radius: 5px + panelPadding + 1px
    color: $bright2
    font-size: 13px

  h1
    display: flex
    align-items: center
    gap: 6px
    color: $bright1
    font-size: 15px
    font-weight: 600
    white-space: nowrap

    svg
      color: $bright2
      margin-right: 2px

  // Same look as the branch labels of the history panel
  .branch
    overflow: hidden
    text-overflow: ellipsis
    padding: 1px 6px
    border-radius: 3px
    border: 1px solid var(--color)
    color: var(--color)
    font-size: 11px
    font-weight: bold

    &.current
      background: var(--color)
      color: $dark2

  section
    display: flex
    flex-direction: column
    min-height: 0

  h2
    display: flex
    align-items: center
    gap: 6px
    margin: 0 0 6px
    color: $bright1
    font-size: 12px
    font-weight: bold
    text-transform: uppercase
    letter-spacing: 0.5px

  // Fixed height pill, so the digits center regardless of the heading's line height
  .count
    display: inline-flex
    align-items: center
    justify-content: center
    box-sizing: border-box
    height: 16px
    min-width: 16px
    padding: 0 5px
    border-radius: 8px
    border: 1px solid $dark1 * 1.5
    background: $dark1
    color: $bright1
    font-size: 10px
    font-weight: 600
    line-height: 1
    letter-spacing: 0
    font-variant-numeric: tabular-nums

  .conflict-count
    margin-left: auto
    color: $warn
    font-size: 11px
    font-weight: normal
    text-transform: none
    letter-spacing: 0

  .note
    color: $bright2

  .changes
    display: grid
    grid-template-columns: minmax(0, 1fr)
    align-items: center
    gap: 2px 4px
    max-height: 280px
    overflow: auto
    padding: 8px
    border-radius: 3px
    background: rgba(black, 0.2)

    &.conflicting
      grid-template-columns: minmax(0, 1fr) 192px

  // Above the conflict choices
  .keep-all
    grid-column: 2
    margin-bottom: 2px

  .feature
    grid-column: 1 / -1
    display: flex
    align-items: center
    gap: 6px
    line-height: 1.8

    svg
      flex: 0 0 auto

    .name
      flex: 1 1 auto
      overflow: hidden
      text-overflow: ellipsis
      white-space: nowrap

    .status
      flex: 0 0 auto
      font-size: 11px

    &.added .status
      color: $green
    &.modified .status
      color: $warn
    &.removed .status
      color: $cancel

    &:hover
      color: $bright1

  .label
    display: flex
    align-items: center
    gap: 6px
    min-width: 0
    overflow: hidden
    text-overflow: ellipsis
    white-space: nowrap
    line-height: 1.8

    &.nested
      display: block
      // Aligned with the feature name
      padding-left: 22px
      font-size: 12px

    svg
      flex: 0 0 auto

    span
      overflow: hidden
      text-overflow: ellipsis

    &:hover
      color: $bright1

  .radio-bar
    :deep(.button)
      flex: 1 1 0
      padding: 2px 6px

  .input
    width: 100%
    box-sizing: border-box
    margin-bottom: 6px

  .actions
    display: flex
    justify-content: flex-end
    gap: 8px

</style>


<script setup>

  import { branchColorMap } from './../js/history-graph.js'

  const props = defineProps(['document'])

  const highlight = inject('highlight')

  const merge = computed(() => props.document.activeMerge )
  const busy = computed(() => props.document.isRegenerating )

  const colors = computed(() => {
    props.document.repositoryVersion
    return branchColorMap(props.document.repository)
  })

  const choices = computed(() => [
    { key: 'theirs', branch: merge.value.branch },
    { key: 'ours', branch: merge.value.current },
  ])

  const conflictsByFeature = computed(() => Object.groupBy(merge.value.conflicts.filter(conflict => conflict.featureId ), conflict => conflict.featureId ))
  const otherConflicts = computed(() => merge.value.conflicts.filter(conflict => !conflict.featureId ))

  // The side chosen for every conflict, if they agree
  const sideEverywhere = computed(() => choices.value.find(side =>
    merge.value.conflicts.every(conflict => merge.value.resolutions[conflict.key] == side.key )
  )?.key )

  function sideItems(label, title) {
    return Object.fromEntries(choices.value.map(side => [side.key, { label: label(side), title: title(side) }] ))
  }

  function resolve(conflicts, side) {
    if(busy.value) return
    const keys = conflicts.filter(conflict => merge.value.resolutions[conflict.key] != side ).map(conflict => conflict.key )
    if(keys.length) merge.value.resolve(keys, side)
  }

  function highlightFeature(id) {
    highlight.value = props.document.timeline.features.find(feature => feature.id == id ) || null
  }

  function commit() {
    if(busy.value) return
    merge.value.commit()
  }

</script>
