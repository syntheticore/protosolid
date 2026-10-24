import Serialize from './serialize.js'
import Expression from './expression.js'
import preferences from '../preferences.js'

// Turns a merge conflict from Repository.planMerge into a readable row such as
// { featureId: 'id-16', label: 'Distance', ours: '3mm', theirs: '1mm' }.
// Conflicts outside of features have no featureId, but an icon and a self-contained label.
// Conflicts of features are listed under the change of their feature instead.
export function describeConflict(conflict, timeline) {
  const { path, ours, theirs } = conflict
  const featureId = conflictFeatureId(path)

  if(featureId) {
    const [, , , group, key, ...deeper] = path
    const feature = timeline.features.find(feature => feature.id == featureId )
    if(!group) {
      return { featureId, label: 'Whole feature', ours: ours === undefined ? 'deleted' : 'modified', theirs: theirs === undefined ? 'deleted' : 'modified' }
    }
    const setting = (group == 'values' || group == 'expressions') && feature?.settings?.[key]
    if(setting) {
      const label = setting.title + (group == 'expressions' ? ' expression' : '')
      const format = value => deeper.length ? 'modified' : group == 'expressions' ? formatValue(value) : formatSetting(setting, value)
      return { featureId, label, ours: format(ours), theirs: format(theirs) }
    }
    return { featureId, label: describePath(path.slice(3)), ours: formatValue(ours), theirs: formatValue(theirs) }
  }

  if(path.at(-1) == '$order') {
    return { icon: 'sort', label: `${describePath(path.slice(0, -1))}: order`, ours: 'reordered', theirs: 'reordered' }
  }

  return { icon: 'pen', label: describePath(path), ours: formatValue(ours), theirs: formatValue(theirs) }
}

// Features the merge adds, modifies or removes compared to the current version,
// in timeline order. Features with conflicts are always listed, their status is
// null when the chosen resolutions leave them unchanged.
// Returns rows like { id, status: 'added', icon: 'extrude', name: 'Extrude 12' }.
export function describeFeatureChanges(before, after, conflicts) {
  const conflicted = new Map()
  conflicts.forEach(({ path, ours, theirs }) => {
    const id = conflictFeatureId(path)
    if(id && !conflicted.get(id)) conflicted.set(id, path.length == 3 ? ours || theirs : undefined)
  })
  const beforeFeatures = new Map(before.timeline.features.map(dump => [dump.id, dump] ))
  const afterIds = new Set(after.timeline.features.map(dump => dump.id ))
  const changes = after.timeline.features.flatMap(dump => {
    const previous = beforeFeatures.get(dump.id)
    const same = previous && JSON.stringify(previous) == JSON.stringify(dump)
    if(same && !conflicted.has(dump.id)) return []
    return [{ id: dump.id, status: !previous ? 'added' : same ? null : 'modified', ...describeFeature(dump.id, dump) }]
  })
  before.timeline.features
    .filter(dump => !afterIds.has(dump.id) )
    .forEach(dump => changes.push({ id: dump.id, status: 'removed', ...describeFeature(dump.id, dump) }) )
  // Deleted on one side and modified on the other, while the deletion wins
  conflicted.forEach((dump, id) => {
    if(!afterIds.has(id) && !beforeFeatures.has(id)) changes.push({ id, status: null, ...describeFeature(id, dump) })
  })
  return changes
}

function conflictFeatureId(path) {
  const [section, list, item] = path
  return section == 'timeline' && list == 'features' && item?.id
}

function describeFeature(id, dump) {
  const title = dump.title || humanize(dump.$class.replace(/Feature$/, ''))
  return { icon: Serialize.classes[dump.$class]?.icon || 'cube', name: `${title} ${shortId(id)}` }
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
