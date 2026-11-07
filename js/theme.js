import { createNanoEvents } from 'nanoevents'

import preferences, { emitter as prefEmitter } from './preferences.js'

// Resolves the theme preference to 'light' or 'dark' and exposes it to the
// styles through the data-theme attribute of the root element.
// In Electron, the system preference reflects nativeTheme.themeSource.

const emitter = createNanoEvents()
const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')

export function currentTheme() {
  if(preferences.theme === 'system') return darkQuery.matches ? 'dark' : 'light'
  return preferences.theme
}

function applyTheme() {
  const theme = currentTheme()
  if(document.documentElement.dataset.theme === theme) return
  document.documentElement.dataset.theme = theme
  emitter.emit('changed', theme)
}

export function onThemeChange(callback) {
  return emitter.on('changed', callback)
}

applyTheme()
prefEmitter.on('updated', applyTheme)
darkQuery.addEventListener('change', applyTheme)
