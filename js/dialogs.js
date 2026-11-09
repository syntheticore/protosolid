import { reactive } from 'vue'

// Desktop web views do not implement window.prompt, so text input goes through an
// in-app dialog rendered by DialogView.
export const dialogState = reactive({ current: null })

// Resolves with { action, values } or null when canceled
export function showDialog({ title, icon, message, fields = [], actions }) {
  if(dialogState.current) closeDialog(null)
  return new Promise(resolve => {
    dialogState.current = {
      title,
      icon,
      message,
      fields: fields.map(field => ({ value: '', ...field })),
      actions: actions || [{ label: 'OK', value: 'ok', primary: true }],
      resolve,
    }
  })
}

export function closeDialog(result) {
  const dialog = dialogState.current
  dialogState.current = null
  dialog.resolve(result)
}

// Resolves with the entered text or null when canceled
export async function requestText({ title, icon, message, label, value = '', placeholder = '', confirm = 'OK' }) {
  const result = await showDialog({
    title,
    icon,
    message,
    fields: [{ key: 'text', label, value, placeholder, required: true }],
    actions: [{ label: confirm, value: 'ok', primary: true }],
  })
  return result && result.values.text.trim()
}
