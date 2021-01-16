import { createNanoEvents } from 'nanoevents'

const emitter = createNanoEvents()

const preferences = {
  preferredUnit: 'mm',
  highDPI: true,
  shadowMaps: true,
  blurredOverlays: true,
}

function savePreferences() {
  window.localStorage.setItem('preferences', JSON.stringify(preferences))
  console.log(preferences)
  emitter.emit('updated')
}

function loadPreferences() {
  const prefs = JSON.parse(window.localStorage.getItem('preferences'))
  for(const pref in prefs) {
    preferences[pref] = prefs[pref]
  }
  console.log(preferences)
}

export { emitter, savePreferences, loadPreferences }

export default preferences
