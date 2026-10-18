import Serialize from './serialize.js'
import Expression from './expression.js'
import preferences from '../preferences.js'

// Turns a merge conflict from Repository.planMerge into a readable row such as
// { icon: 'chamfer', text: 'Chamfer 16: Distance = 1mm (other) | 3mm (master)' }.
// Branches are the names of the current and the merged branch.
export function describeConflict(conflict, timeline, branches) {
  const { path, ours, theirs } = conflict
  const [section, list, item, ...rest] = path

  if(section == 'timeline' && list == 'features' && item?.id) {
    const feature = timeline.features.find(feature => feature.id == item.id )
    const dump = ours || theirs
    const featureClass = feature?.constructor || Serialize.classes[dump?.$class]
    const name = `${feature?.title || humanize(dump?.$class?.replace(/Feature$/, '') || 'Feature')} ${shortId(item.id)}`
    const icon = featureClass?.icon || 'cube'
    if(!rest.length) {
      return { icon, text: `${name}: ${pair([ours === undefined ? 'deleted' : 'modified', theirs === undefined ? 'deleted' : 'modified'], branches)}` }
    }
    const [group, key, ...deeper] = rest
    const setting = (group == 'values' || group == 'expressions') && feature?.settings?.[key]
    if(setting) {
      const label = setting.title + (group == 'expressions' ? ' expression' : '')
      const format = value => group == 'expressions' ? formatValue(value) : formatSetting(setting, value)
      const values = deeper.length ? ['modified', 'modified'] : [format(ours), format(theirs)]
      return { icon, text: `${name}: ${label} = ${pair(values, branches)}` }
    }
    return { icon, text: `${name}: ${describePath(rest)} = ${pair([formatValue(ours), formatValue(theirs)], branches)}` }
  }

  if(path.at(-1) == '$order') {
    return { icon: 'sort', text: `${describePath(path.slice(0, -1))}: order changed in both branches` }
  }

  return { icon: 'pen', text: `${describePath(path)} = ${pair([formatValue(ours), formatValue(theirs)], branches)}` }
}

// Merged branch left, current branch right, like the dialog's buttons
function pair(values, branches) {
  return values.map((value, i) => `${value} (${branches[i]})` ).reverse().join(' | ')
}

function formatSetting(setting, value) {
  if(value === undefined) return 'deleted'
  if(typeof value == 'number') {
    if(setting.type == 'length') return new Expression(value).format(preferences.preferredUnit)
    if(setting.type == 'angle') return new Expression(value, [], 'angle').format('°')
  }
  if(setting.options?.[value]) return setting.options[value].title
  if(setting.type == 'bool') return value ? 'On' : 'Off'
  return formatValue(value)
}

function formatValue(value) {
  if(value === undefined) return 'deleted'
  if(value === null) return 'none'
  if(typeof value == 'number') return String(Number(value.toFixed(3)))
  if(typeof value == 'boolean') return value ? 'On' : 'Off'
  if(typeof value == 'string') return value
  if(value.$class) return humanize(value.$class)
  return 'modified'
}

function describePath(path) {
  return path
    .filter(segment => segment != 'values' )
    .map(segment => typeof segment == 'string' ? humanize(segment) : `#${shortId(segment.id)}` )
    .join(' › ')
}

function shortId(id) {
  return String(id).replace(/^id-/, '')
}

function humanize(name) {
  const words = name.replace(/^_/, '').replace(/([a-z0-9])([A-Z])/g, '$1 $2').toLowerCase()
  return words.charAt(0).toUpperCase() + words.slice(1)
}
