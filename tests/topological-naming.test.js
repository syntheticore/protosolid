import assert from 'node:assert/strict'
import test from 'node:test'
import { nameTopology, propagateTopology, resolveTopology, topologySelection, withTopologyNaming,
  initializeTopologyOrigins, captureTopologySelection, restoreTopologySelections, namespaceTopology } from '../js/core/topological-naming.js'

const entity = kind => ({ topologyKind: kind, neighbors: [] })
const neighbors = item => item.neighbors
const name = (item, role) => nameTopology(item, item.topologyKind, role)

test('split selection uses named incidence, independent of output order and allocation', () => {
  const feature = { id: 'producer' }
  const source = entity('face'), leftBoundary = entity('edge'), rightBoundary = entity('edge')
  withTopologyNaming(feature, () => { name(source, 'cap'); name(leftBoundary, 'left'); name(rightBoundary, 'right') })
  const build = order => {
    const left = entity('face'), right = entity('face')
    left.neighbors = [leftBoundary]
    right.neighbors = [rightBoundary]
    const outputs = order ? [left, right] : [right, left]
    propagateTopology(outputs, [{ source, relation: 'modified', outputs }], 'split', neighbors)
    return { left, outputs }
  }
  const first = build(true)
  const selection = JSON.parse(JSON.stringify(topologySelection(first.left, neighbors)))
  const second = build(false)
  const resolved = resolveTopology(second.outputs, selection, neighbors)
  assert.equal(resolved.status, 'resolved')
  assert.deepEqual(resolved.items, [second.left])
})

test('ancestry follows multiple inserted operations, merges and deletion without retargeting', () => {
  const source = entity('edge'), sibling = entity('edge')
  withTopologyNaming({ id: 'producer' }, () => { name(source, 'selected'); name(sibling, 'unrelated') })
  const selection = topologySelection(source, neighbors)
  let current = source
  for(const id of ['inserted', 'modified']) {
    const output = entity('edge')
    withTopologyNaming({ id }, () => propagateTopology([output], [
      { source: current, relation: 'modified', outputs: [output] },
    ], 'modify', neighbors))
    current = output
  }
  const merged = entity('edge')
  withTopologyNaming({ id: 'merge' }, () => propagateTopology([merged], [
    { source: current, relation: 'modified', outputs: [merged] },
    { source: sibling, relation: 'modified', outputs: [merged] },
  ], 'merge', neighbors))
  assert.deepEqual(resolveTopology([merged], selection, neighbors).items, [merged])
  assert.equal(resolveTopology([sibling], selection, neighbors).status, 'deleted')
})

test('a symmetric split reports ambiguity; all-descendants is explicit', () => {
  const source = entity('face')
  withTopologyNaming({ id: 'producer' }, () => name(source, 'face'))
  const selection = topologySelection(source, neighbors)
  const outputs = [entity('face'), entity('face')]
  propagateTopology(outputs, [{ source, relation: 'modified', outputs }], 'split', neighbors)
  assert.deepEqual(resolveTopology(outputs, selection, neighbors), { status: 'ambiguous', items: [] })
  assert.deepEqual(resolveTopology(outputs, { ...selection, policy: 'all-descendants' }, neighbors).items, outputs)
})

test('an explicit pick distinguishes symmetric cells by support side without retargeting a missing side', () => {
  const producer = { id: 'bore' }
  const supportSide = item => [{ support: ['bore-wall'], frame: 'axis', axis: 2, side: item.side }]
  const build = (owner, order) => withTopologyNaming(owner, () => {
    const items = order.map(side => Object.assign(entity('edge'), { side }))
    items.forEach(item => name(item, 'intersection'))
    initializeTopologyOrigins(items, neighbors, supportSide)
    restoreTopologySelections(owner, items, neighbors, supportSide)
    return items
  })
  const initial = build(producer, [-1, 1])
  const refs = initial.map(item => captureTopologySelection(item))
  assert.notEqual(refs[0].identity, refs[1].identity)
  const restored = JSON.parse(JSON.stringify(producer))
  const rebuilt = build(restored, [1, -1])
  for(let i = 0; i < refs.length; i++) {
    assert.equal(resolveTopology(rebuilt, refs[i], neighbors).items[0].side, initial[i].side)
  }
  const surviving = build(restored, [1])
  assert.equal(resolveTopology(surviving, refs[0], neighbors).status, 'deleted')
  assert.equal(resolveTopology(surviving, refs[1], neighbors).items[0], surviving[0])
})

test('construction bindings survive serialization and reordered creation with a preceding feature', () => {
  const feature = { id: 'existing' }
  const a = entity('face'), b = entity('face')
  withTopologyNaming(feature, () => { name(a, 'start'); name(b, 'end') })
  const restored = JSON.parse(JSON.stringify(feature))
  withTopologyNaming({ id: 'inserted-before-existing' }, () => name(entity('face'), 'start'))
  const nextA = entity('face'), nextB = entity('face')
  withTopologyNaming(restored, () => { name(nextB, 'end'); name(nextA, 'start') })
  assert.equal(nextA.topology.identity, a.topology.identity)
  assert.equal(nextB.topology.identity, b.topology.identity)
  assert.equal(restored.topologyNames.bindings.length, 2)
})

test('semantic branches retain identity when a side disappears and selection policies remain distinct', () => {
  const producer = { id: 'producer' }, split = { id: 'split' }
  const source = entity('body')
  withTopologyNaming(producer, () => {
    name(source, 'body')
    initializeTopologyOrigins([source], neighbors)
  })
  const upstream = captureTopologySelection(source)
  const descendants = { ...upstream, policy: 'all-descendants' }
  const build = (owner, sides) => withTopologyNaming(owner, () => {
    const outputs = sides.map(side => Object.assign(entity('body'), { side }))
    propagateTopology(outputs, [{ source, relation: 'modified', outputs }],
      item => ({ name: 'split', branch: item.side }), neighbors)
    initializeTopologyOrigins(outputs, neighbors)
    restoreTopologySelections(owner, outputs, neighbors)
    return outputs
  })
  const initial = build(split, ['left', 'right'])
  const left = captureTopologySelection(initial[0]), right = captureTopologySelection(initial[1])
  const ids = initial.map(item => item.topology.identity)
  assert.equal(resolveTopology(initial, upstream, neighbors).status, 'ambiguous')
  assert.deepEqual(resolveTopology(initial, descendants, neighbors).items, initial)
  const saved = JSON.parse(JSON.stringify(split))
  const onlyLeft = build(saved, ['left'])
  assert.equal(onlyLeft[0].topology.identity, ids[0])
  assert.equal(resolveTopology(onlyLeft, left, neighbors).status, 'resolved')
  assert.equal(resolveTopology(onlyLeft, right, neighbors).status, 'deleted')
  const returned = build(saved, ['right', 'left'])
  assert.deepEqual(returned.map(item => item.topology.identity), ids.toReversed())
  assert.deepEqual(resolveTopology(returned, right, neighbors).items, [returned[0]])
})

test('unnamed branches require full boundary evidence and cannot both bind to a sole result', () => {
  const producer = { id: 'producer' }, modifier = { id: 'modifier' }
  const source = entity('body')
  const boundaries = ['left', 'left-end', 'right'].map(role => entity('face'))
  withTopologyNaming(producer, () => {
    name(source, 'body')
    boundaries.forEach((boundary, index) => name(boundary, ['left', 'left-end', 'right'][index]))
    initializeTopologyOrigins([source, ...boundaries], neighbors)
  })
  const build = regions => withTopologyNaming(modifier, () => {
    const outputs = regions.map(indices => Object.assign(entity('body'), {
      neighbors: indices.map(index => boundaries[index]),
    }))
    propagateTopology(outputs, [{ source, relation: 'modified', outputs }], 'cut', neighbors)
    initializeTopologyOrigins([...outputs, ...boundaries], neighbors)
    restoreTopologySelections(modifier, outputs, neighbors)
    return outputs
  })
  const initial = build([[0, 1], [2]])
  const left = captureTopologySelection(initial[0]), right = captureTopologySelection(initial[1])
  const leftId = initial[0].topology.identity
  const surviving = build([[0, 1]])
  assert.equal(surviving[0].topology.identity, leftId)
  assert.equal(resolveTopology(surviving, left, neighbors).status, 'resolved')
  assert.equal(resolveTopology(surviving, right, neighbors).status, 'deleted')
  const insufficient = build([[0]])
  assert.equal(resolveTopology(insufficient, left, neighbors).status, 'deleted')
  const conflicting = build([[0, 1, 2]])
  for(const selection of [left, right]) {
    assert.equal(resolveTopology(conflicting, selection, neighbors).status, 'deleted')
  }
  assert.throws(() => captureTopologySelection(conflicting[0]), /cannot be distinguished/)
})

test('copies establish independent identities and merges retain all logical parents', () => {
  const producer = { id: 'producer' }, copier = { id: 'copy' }, merger = { id: 'merge' }
  const source = entity('body')
  withTopologyNaming(producer, () => {
    name(source, 'body')
    initializeTopologyOrigins([source], neighbors)
  })
  const sourceRef = captureTopologySelection(source)
  const copy = entity('body')
  withTopologyNaming(copier, () => {
    propagateTopology([copy], [{ source, relation: 'modified', outputs: [copy] }], 'transform', neighbors)
    namespaceTopology([copy], 'grid/1/0')
    initializeTopologyOrigins([copy], neighbors)
  })
  const copyRef = captureTopologySelection(copy)
  assert.equal(resolveTopology([copy], sourceRef, neighbors).status, 'deleted')
  assert.equal(resolveTopology([source], copyRef, neighbors).status, 'deleted')
  assert.notEqual(copy.topology.identity, source.topology.identity)
  const merged = entity('body')
  withTopologyNaming(merger, () => {
    propagateTopology([merged], [source, copy].map(source => ({ source, relation: 'modified', outputs: [merged] })), 'join', neighbors)
    initializeTopologyOrigins([merged], neighbors)
  })
  const mergedRef = captureTopologySelection(merged)
  for(const selection of [sourceRef, copyRef]) {
    assert.equal(resolveTopology([merged], selection, neighbors).status, 'resolved')
  }
  assert.ok(merged.topology.ancestors.includes(source.topology.identity))
  assert.ok(merged.topology.ancestors.includes(copy.topology.identity))
  assert.equal(resolveTopology([source], mergedRef, neighbors).status, 'deleted')
})
