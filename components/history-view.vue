<template lang="pug">

  .history-view

    .empty(v-if="!graph")
      p This document has no version history yet.
      p Save the document to record its first version.

    template(v-else)

      .changes(v-if="document.hasUnsavedChanges") Unsaved changes

      .scroller

        .canvas

          svg(:width="graph.width" :height="graph.rows.length * rowHeight" xmlns="http://www.w3.org/2000/svg")

            polyline(
              v-for="edge in graph.edges"
              :points="edge.points"
              :stroke="getColor(edge.lane)"
              stroke-width="2"
              stroke-linejoin="round"
              fill="none"
            )

            circle.dot(
              v-for="row in graph.rows"
              :cx="getX(row.lane)"
              :cy="getY(row.index)"
              :r="row.id == graph.headId ? 6.5 : 5.5"
              :class="{ head: row.id == graph.headId }" :fill="getColor(row.lane)"
            )

        ul.commits

          li.commit(
            v-for="row in graph.rows"
            :class="{ head: row.id == graph.headId }"
            :title="row.message + '\\n' + new Date(row.date).toLocaleString()"
          )

            span.branch(
              v-for="name in row.branches"
              :class="{ current: name == graph.currentBranch }"
              :style="{ '--color': getColor(row.lane) }"
              title="Click to switch to this branch, double click to rename it"
              @click="checkout(name)"
              @dblclick="renameBranch(name)"
            ) {{ name }}

            .title {{ row.message }}

            .date {{ formatDate(row.date) }}

            .actions(v-if="!document.activeMerge")

              button(
                v-if="row.id != graph.headId"
                title="Open this version"
                @click="checkout(row.id)"
              )
                Icon(icon="eye" fixed-width)

              button(title="Start a new branch from this version" @click="createBranch(row.id)")
                Icon(icon="code-branch" fixed-width)

              template(v-for="name in row.branches.filter(name => name != graph.currentBranch)")

                button(
                  v-if="graph.currentBranch"
                  :title="`Merge '${name}' into '${graph.currentBranch}'`"
                  @click="merge(name)"
                )
                  Icon(icon="compress-alt" fixed-width)

                button(:title="`Delete branch '${name}'`" @click="deleteBranch(name)")
                  Icon(icon="trash-alt" fixed-width)

</template>


<style lang="stylus" scoped>

  .history-view
    padding: 0.25rem

  .empty
    padding: 0.5rem 0.75rem
    line-height: 1.5
    white-space: normal
    width: 310px

    p + p
      margin-top: 0.5rem
      color: bright2()

  .changes
    padding: 0.375rem 0.75rem 0.5rem
    color: var(--warn)

  // Fixed width, so revealing the row actions on hover never resizes the panel
  .scroller
    width: 360px
    display: flex
    align-items: flex-start
    max-height: 60vh
    overflow-y: auto

  .canvas
    flex: 0 0 auto
    background: tint(0.05)
    border-radius: 2px
    display: flex

  .dot
    stroke: dark1()
    stroke-width: 2

    &.head
      fill: var(--fg)

  .commits
    flex: 1 1 auto
    min-width: 0

  .commit
    display: flex
    align-items: center
    height: 32px
    gap: 0.5rem
    padding-left: 0.75rem
    // Matches the space above and below the action buttons
    padding-right: 4px
    white-space: nowrap

    &:not(:last-child)
      border-bottom: 1px solid dark1()

    &:hover
      background: dark1()

      .actions
        display: flex

      .date
        display: none

    &.head .title
      color: var(--fg)
      font-weight: bold

  .branch
    flex: 0 1 auto
    max-width: 120px
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
      color: #1e1e23 // Dark on the colored fill in either theme

  .title
    flex: 1 1 auto
    overflow: hidden
    text-overflow: ellipsis

  .date
    flex: 0 0 auto
    margin-right: calc(0.75rem - 4px)
    color: bright2()
    font-size: 11px

  .actions
    display: none
    flex: 0 0 auto
    gap: 2px

    button
      display: flex
      align-items: center
      justify-content: center
      width: 24px
      height: 24px
      background: none
      border: none
      padding: 0
      border-radius: 3px
      color: bright2()

      &:hover
        color: var(--fg)
        background: tint(0.12)

</style>


<script setup>

  import { requestText } from './../js/dialogs.js'
  import { layoutHistory, laneColor } from './../js/history-graph.js'

  const props = defineProps(['document'])
  const bus = inject('bus')

  const rowHeight = 32
  const laneWidth = 16

  const getX = lane => 14 + lane * laneWidth
  const getY = index => rowHeight / 2 + index * rowHeight
  const getColor = laneColor

  const graph = computed(() => {
    props.document.repositoryVersion
    const repository = props.document.repository
    if(!repository || repository.isEmpty) return null

    const { rows, edges: pending } = layoutHistory(repository)

    const byId = new Map(rows.map(row => [row.id, row]))
    const edges = pending.map(({ child, parent, lane }) => {
      const from = byId.get(child)
      const to = byId.get(parent)
      const points = [[getX(from.lane), getY(from.index)]]
      if(lane != from.lane) points.push([getX(lane), getY(from.index + 1)])
      if(lane != to.lane) points.push([getX(lane), getY(to.index - 1)])
      points.push([getX(to.lane), getY(to.index)])
      return { lane, points: points.map(point => point.join(',') ).join(' ') }
    })

    const laneCount = Math.max(...rows.map(row => row.lane ), ...pending.map(edge => edge.lane )) + 1
    return {
      rows,
      edges,
      width: getX(laneCount - 1) + 14,
      headId: repository.headId,
      currentBranch: repository.currentBranch,
    }
  })

  function formatDate(iso) {
    const date = new Date(iso)
    const minutes = Math.round((Date.now() - date) / 60000)
    if(minutes < 1) return 'just now'
    if(minutes < 60) return `${minutes} min ago`
    if(minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`
    if(minutes < 60 * 24 * 7) return `${Math.round(minutes / 60 / 24)} d ago`
    return date.toLocaleDateString()
  }

  async function run(action) {
    try {
      await action()
    } catch(error) {
      console.error(error)
      alert(error.message || error)
    }
  }

  function checkout(target) {
    if(target == props.document.repository.currentBranch || props.document.activeMerge) return
    run(() => props.document.checkoutVersion(target) )
  }

  async function createBranch(commitId) {
    const name = await requestText({
      title: 'New Branch',
      icon: 'code-branch',
      message: 'Start a new branch from this version.',
      placeholder: 'Branch name',
      confirm: 'Create Branch',
    })
    if(name) run(() => props.document.createBranch(name, commitId) )
  }

  async function renameBranch(name) {
    if(props.document.activeMerge) return
    const newName = await requestText({
      title: 'Rename Branch',
      icon: 'code-branch',
      placeholder: 'Branch name',
      value: name,
      confirm: 'Rename',
    })
    if(newName && newName != name) run(() => props.document.renameBranch(name, newName) )
  }

  function deleteBranch(name) {
    if(!window.confirm(`Delete branch '${name}'? Versions that are not part of another branch will be lost.`)) return
    run(() => props.document.deleteBranch(name) )
  }

  function merge(name) {
    // The merge preview needs the viewport
    bus.emit('close-widgets')
    run(() => props.document.mergeBranch(name) )
  }

</script>
