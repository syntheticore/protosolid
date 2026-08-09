import { createNanoEvents } from 'nanoevents'

const emitter = createNanoEvents()

const preferences = {
  preferredUnit: 'mm',
  modelingKernel: 'shapex',
  highDPI: true,
  antiAlias: true,
  shadowMaps: true,
  blurredOverlays: true,
  zoomToCursor: true,
  cacheTesselation: true,
}

function savePreferences() {
  window.localStorage.setItem('preferences', JSON.stringify(preferences))
  console.log(preferences)
  emitter.emit('updated')
}

function loadPreferences() {
  const prefs = JSON.parse(window.localStorage.getItem('preferences') || '{}')
  for(const pref in prefs) {
    if(pref in preferences) preferences[pref] = prefs[pref]
  }
  console.log(preferences)
}

export { emitter, savePreferences, loadPreferences }

export default preferences
