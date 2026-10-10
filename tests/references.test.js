import assert from 'node:assert/strict'
import test from 'node:test'
import { nameTopology, propagateTopology, resolveTopology, topologySelection, withTopologyNaming } from '../js/core/topological-naming.js'

import './browser-env.mjs'
const { FaceReference, EdgeReference, SolidReference } = await import('../js/core/references.js')

const neighbors = item => item.neighbors
function tree(items) {
  return { findChild: id => id == 'component' ? {
    compound: { resolveTopology: selection => resolveTopology(items, selection, neighbors) },
  } : undefined }
}

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
