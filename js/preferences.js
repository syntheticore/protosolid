import { createNanoEvents } from 'nanoevents'

const emitter = createNanoEvents()

const preferences = {
  preferredUnit: 'mm',
  angleUnit: 'degrees',
  densityUnit: 'g/cm3',
  theme: 'system',
  highDPI: true,
  antiAlias: true,
  shadowMaps: true,
  blurredOverlays: true,
  timelineFeatureVisibility: 'all',
  zoomToCursor: true,
  cacheTesselation: true,
  centerOfMassTolerance: '0.01mm',
  tessellationTolerance: '0.4mm',
  tessellationAngularTolerance: '0.2rad',
}

function savePreferences() {
  window.localStorage.setItem('preferences', JSON.stringify(preferences))
  emitter.emit('updated')
  if(window.ipc) window.ipc.send('preferences-updated', { ...preferences })
}

function loadPreferences() {
  try {
    const prefs = JSON.parse(window.localStorage.getItem('preferences')) || {}
    // The old value was only a placeholder and was never applied to viewport meshes.
    if(prefs.tessellationTolerance === '0.0001mm') prefs.tessellationTolerance = '0.4mm'
    applyPreferences(prefs)
  } catch(error) {
    console.warn('Could not load preferences', error)
  }
}

function applyPreferences(prefs) {
  for(const pref in prefs) {
    if(Object.hasOwn(preferences, pref)) preferences[pref] = prefs[pref]
  }
  emitter.emit('updated')
}

window.addEventListener('storage', event => {
  if(event.key !== 'preferences' || !event.newValue) return
  try {
    applyPreferences(JSON.parse(event.newValue))
  } catch(error) {
    console.warn('Could not synchronize preferences', error)
  }
})

export { emitter, savePreferences, loadPreferences, applyPreferences }

export default preferences
