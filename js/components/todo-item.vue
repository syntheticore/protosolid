<template lang="pug">
span
  | {{todo}}
</template>

<script>
export default {
  name: 'TodoItem',
  props: ['todo'],
}
</script>
