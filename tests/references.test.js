import assert from 'node:assert/strict'
import test from 'node:test'
import { nameTopology, propagateTopology, resolveTopology, topologySelection, withTopologyNaming } from '../js/core/topological-naming.js'

import './browser-env.mjs'
const { FaceReference, EdgeReference, SolidReference } = await import('../js/core/references.js')
const { Feature, FilletFeature } = await import('../js/core/features.js')

const neighbors = item => item.neighbors
function tree(items) {
  return { findChild: id => id == 'component' ? {
    compound: { resolveTopology: selection => resolveTopology(items, selection, neighbors) },
  } : undefined }
}

for(const failure of ['deleted', 'ambiguous']) {
  test(`an explicit edge pick replaces ${failure} references while retaining valid selections`, () => {
    const lost = { id: 'lost', topologyKind: 'edge', neighbors: [] }
    const kept = { id: 'kept', topologyKind: 'edge', neighbors: [] }
    const replacement = { id: 'replacement', topologyKind: 'edge', neighbors: [] }
    withTopologyNaming({ id: 'producer' }, () => {
      nameTopology(lost, 'edge', 'lost')
      nameTopology(kept, 'edge', 'kept')
      nameTopology(replacement, 'edge', 'replacement')
    })
    const reference = item => new EdgeReference(null, 'component', topologySelection(item, neighbors))
    const lostRef = reference(lost)
    const keptRef = reference(kept)
    lostRef.update(tree([lost, kept]))
    const branches = failure == 'ambiguous' ? [0, 1].map(id => ({ id, topologyKind: 'edge', neighbors: [] })) : []
    propagateTopology(branches, [{ source: lost, relation: 'modified', outputs: branches }], 'split', neighbors)
    const upstream = tree([...branches, kept, replacement])
    const document = {
      activeComponent: { id: 'component', sourceId: () => 'component' },
      timeline: { features: [] },
      top: index => {
        assert.equal(index, 0, 'picking resolves against the state before the edited feature')
        return upstream
      },
    }
    const feature = new Feature(document, false, 'Edge picker fixture', { edges: { type: 'edge', multi: true } })
    document.timeline.features.push(feature)
    feature.edges = () => [lostRef, keptRef]
    feature.updateReferences(upstream)
    assert.match(feature.error.msg, new RegExp(failure))
    assert.deepEqual(feature.edges(), [lostRef, keptRef], 'regeneration preserves failed references')

    const replacementRef = reference(replacement)
    feature.selectInput('edges', replacementRef, replacement)
    assert.deepEqual(feature.edges(), [keptRef, replacementRef])
    assert.deepEqual(feature.updateReferences(upstream).edges, [kept, replacement])
    assert.equal(feature.error, undefined)

    feature.selectInput('edges', reference(kept), kept)
    assert.deepEqual(feature.edges(), [replacementRef], 'valid selections can still be toggled off')
  })
}

test('canceling an edge repair restores the failed fillet without clearing its cached gizmo input', () => {
  const lost = { id: 'lost', topologyKind: 'edge', neighbors: [], center: () => ({ x: 0, y: 0, z: 0 }) }
  const replacement = { id: 'replacement', topologyKind: 'edge', neighbors: [] }
  withTopologyNaming({ id: 'producer' }, () => {
    nameTopology(lost, 'edge', 'lost')
    nameTopology(replacement, 'edge', 'replacement')
  })
  const reference = item => new EdgeReference(null, 'component', topologySelection(item, neighbors))
  const lostRef = reference(lost)
  lostRef.update(tree([lost]))
  const upstream = tree([replacement])
  const document = {
    activeComponent: { id: 'component', sourceId: () => 'component' },
    timeline: { features: [] },
    top: () => upstream,
  }
  const feature = new FilletFeature(document)
  document.timeline.features.push(feature)
  feature.edges = () => [lostRef]
  feature.updateReferences(upstream)
  const originalValues = feature.getValues()

  feature.selectInput('edges', reference(replacement), replacement)
  feature.setValues(originalValues)
  feature.updateReferences(upstream)
  assert.match(feature.error.msg, /deleted/)
  assert.deepEqual(feature.edges(), [lostRef])
  assert.doesNotThrow(() => feature.getGizmo())
})

for(const [Ref, kind] of [[FaceReference, 'face'], [EdgeReference, 'edge'], [SolidReference, 'body']]) {
  test(`${Ref.name} binds ancestry across bodies without a positional hint`, () => {
    const original = { topologyKind: kind, neighbors: [] }
    withTopologyNaming({ id: 'producer' }, () => nameTopology(original, kind, 'selected'))
    const selection = topologySelection(original, neighbors)
    const ref = new Ref(null, 'component', selection)
    const target = { topologyKind: kind, neighbors: [] }
    propagateTopology([target], [{ source: original, relation: 'modified', outputs: [target] }], 'move', neighbors)
    assert.equal(ref.update(tree([target])), undefined)
    assert.equal(ref.getItem(), target)
    const restored = Ref.undump(JSON.parse(JSON.stringify(ref.dump())))
    assert.equal(restored.update(tree([target])), undefined)
    assert.equal(restored.getItem(), target)
    assert.deepEqual(restored.dump(), { componentId: 'component', selection })
    assert.equal(restored.update(tree([]))?.type, 'error')
    assert.equal(restored.getItem(), null)
  })

  test(`${Ref.name} does not retain a stale binding or choose the first split descendant`, () => {
    const source = { topologyKind: kind, neighbors: [] }
    withTopologyNaming({ id: 'producer' }, () => nameTopology(source, kind, 'selected'))
    const ref = new Ref(null, 'component', topologySelection(source, neighbors))
    assert.equal(ref.update(tree([source])), undefined)
    const outputs = [0, 1].map(() => ({ topologyKind: kind, neighbors: [] }))
    propagateTopology(outputs, [{ source, relation: 'modified', outputs }], 'split', neighbors)
    assert.match(ref.update(tree(outputs)).msg, /ambiguous/)
    assert.equal(ref.getItem(), null)
    assert.deepEqual(ref.getItems(), [])
    assert.equal(ref.update({ findChild: () => undefined })?.type, 'error')
  })
}
