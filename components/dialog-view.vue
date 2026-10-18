<template lang="pug">

  transition(name="fade")

    .dialog-view(v-if="dialog" @keydown.stop="onKeyDown" @pointerdown.self="cancel")

      form.dialog.bordered(ref="form" @submit.prevent="submit(primaryAction)")

        h1
          Icon(v-if="dialog.icon" :icon="dialog.icon" fixed-width)
          | {{ dialog.title }}

        p(v-if="dialog.message") {{ dialog.message }}

        ul.details(v-if="dialog.details.length")
          li(v-for="detail in dialog.details")
            Icon(v-if="detail.icon" :icon="detail.icon" fixed-width)
            span {{ detail.text }}

        label(v-for="(field, i) in dialog.fields")
          span(v-if="field.label") {{ field.label }}
          input.input.large(
            type="text"
            v-model="field.value"
            :placeholder="field.placeholder"
            :ref="el => { if(i == 0) firstInput = el }"
          )

        .actions
          button.button.large(type="button" @click="cancel") Cancel
          button.button.large(
            v-for="action in dialog.actions"
            :type="action.primary ? 'submit' : 'button'"
            :class="{ primary: action.primary }"
            :disabled="!isValid"
            @click.prevent="submit(action)"
          ) {{ action.label }}

</template>


<style lang="stylus" scoped>

  dialogPadding = 13px

  .dialog-view
    position: fixed
    inset: 0
    z-index: 100
    display: flex
    align-items: center
    justify-content: center
    background: rgba(black, 0.28)
    transition: opacity 0.15s

  .dialog
    width: 440px
    max-width: calc(100vw - 32px)
    padding: dialogPadding
    // Concentric with the 5px button corners: button radius + padding + 1px border
    border-radius: 5px + dialogPadding + 1px
    color: $bright2
    font-size: 12px
    box-shadow: 0 0 0 1px $dark2 * 0.5, 0 8px 32px rgba(black, 0.25)

  h1
    display: flex
    align-items: center
    gap: 8px
    color: $bright1
    font-size: 17px
    font-weight: bold
    margin-bottom: 12px

    svg
      color: $bright2

  p
    line-height: 1.5
    margin-bottom: 10px
    white-space: pre-line

  .details
    max-height: 120px
    overflow: auto
    margin-bottom: 10px
    padding: 6px 8px
    border-radius: 3px
    background: rgba(black, 0.2)

    li
      display: flex
      align-items: center
      gap: 6px
      line-height: 1.8

      svg
        flex: 0 0 auto

  label
    display: flex
    flex-direction: column
    gap: 6px
    margin-bottom: 12px

    .input
      width: 100%
      box-sizing: border-box

  .actions
    display: flex
    justify-content: flex-end
    gap: 8px
    margin-top: 6px

  .fade-enter-from
  .fade-leave-to
    opacity: 0

</style>


<script setup>

  import { dialogState, closeDialog } from './../js/dialogs.js'

  const dialog = computed(() => dialogState.current )
  const firstInput = ref(null)
  const form = ref(null)

  const primaryAction = computed(() => dialog.value.actions.find(action => action.primary ) || dialog.value.actions[0] )

  const isValid = computed(() => dialog.value.fields.every(field => !field.required || field.value.trim() ) )

  watch(dialog, async value => {
    if(!value) return
    await nextTick()
    if(firstInput.value) {
      firstInput.value.focus()
      firstInput.value.select()
    } else {
      form.value.querySelector('.primary').focus()
    }
  })

  function submit(action) {
    if(!isValid.value) return
    const values = Object.fromEntries(dialog.value.fields.map(field => [field.key, field.value] ))
    closeDialog({ action: action.value, values })
  }

  function cancel() {
    closeDialog(null)
  }

  function onKeyDown(e) {
    if(e.key == 'Escape') cancel()
  }

</script>
