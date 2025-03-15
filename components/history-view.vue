<template lang="pug">

  .history-view

    .canvas

      svg(width="58" height="320" viewBox="0 0 58 320" xmlns="http://www.w3.org/2000/svg")

        template(v-for="(commit, i) in commits")

          template(v-for="parent in commit.parents")

            line(
              :x1="getOffset(commit.branch)"
              :y1="16 + i * 32"
              :x2="getOffset(commits.find(c => c.id == parent ).branch)"
              :y2="16 + commits.findIndex(c => c.id == parent ) * 32"
              :stroke="getColor(commit.parents.length > 1 ? commits.find(c => c.id == parent ).branch : commit.branch)"
              stroke-width="2"
            )

          circle(
            :cx="getOffset(commit.branch)"
            :cy="16 + i * 32"
            r="4"
            :fill="getColor(commit.branch)"
            stroke="#2b2b32"
            stroke-width="3"
          )

    ul.commits

      li.commit(v-for="commit in commits")

        Icon(icon="code-branch")

        .title {{ commit.title }}

</template>


<style lang="stylus" scoped>

  .history-view
    padding: 0.25rem
    padding-right: 0
    display: flex

  .canvas
    background: rgba(white, 0.05)
    width: 58px
    border-radius: 2px
    display: flex

  .commit
    display: flex
    align-items: center
    height: 32px
    gap: 0.5rem
    padding-inline: 0.75rem
    white-space: nowrap

    &:not(:last-child)
      border-bottom: 1px solid $dark1

    &:hover
      background: $dark1

    svg
      color: $bright2

</style>


<script setup>

  const props = defineProps([])
  const emit = defineEmits([])

  const branchNames = ref(['master', 'other'])

  function getOffset(branchName) {
    return 16 + branchNames.value.indexOf(branchName) * 24
  }

  function getColor(branchName) {
    return {
      master: 'yellow',
      other: 'pink',
    }[branchName]
  }

  const commits = ref([
    {
      id: 9,
      title: 'Sixth step',
      branch: 'master',
      parents: [8],
    },
    {
      id: 8,
      title: 'Merge commit',
      branch: 'master',
      parents: [6, 7],
      merged: 7,
    },
    {
      id: 7,
      title: 'Fifth in other branch',
      branch: 'other',
      parents: [5],
    },
    {
      id: 6,
      title: 'Fifth step',
      branch: 'master',
      parents: [4],
    },
    {
      id: 5,
      title: 'Fourth in other branch',
      branch: 'other',
      parents: [3],
    },
    {
      id: 4,
      title: 'Fourth step',
      branch: 'master',
      parents: [3],
    },
    {
      id: 3,
      title: 'Third step',
      branch: 'master',
      parents: [2],
    },
    {
      id: 2,
      title: 'Second step',
      branch: 'master',
      parents: [1],
    },
    {
      id: 1,
      title: 'First step',
      branch: 'master',
      parents: [0],
    },
    {
      id: 0,
      title: 'Initial commit',
      branch: 'master',
      parents: [],
    },
  ])

  const branches = computed(() => {
    return branchNames.value.map(name => commits.value.filter(commit => commit.branch == name ) )
  })

</script>
