import assert from 'node:assert/strict'
import test from 'node:test'
import { FaceReference, EdgeReference } from '../js/core/references.js'

const topology = id => ({ isSame: other => other == id })
const solid = (id, items) => ({ id, faces: () => items, edges: () => items })
const tree = solids => ({ findChild: id => id == 'component' ? {
  compound: { solids: () => solids },
} : undefined })

for(const Ref of [FaceReference, EdgeReference]) {
  test(`${Ref.name} survives unrelated solid removal and renumbering`, () => {
    const target = topology('stable-topology')
    const ref = new Ref(null, 'component', 'component/solid/1', 'stable-topology')
    assert.equal(ref.update(tree([
      solid('component/solid/0', [target]),
      solid('component/solid/1', [topology('unrelated')]),
    ])), undefined)
    assert.equal(ref.getItem(), target)
    assert.equal(ref.update(tree([solid('component/solid/0', [target])])), undefined)
    assert.equal(ref.getItem(), target)
    // Keep the saved solid hint so undo can restore the original arrangement.
    assert.equal(ref.dump().solidId, 'component/solid/1')
  })

  test(`${Ref.name} reports missing or ambiguous topology without throwing`, () => {
    const ref = new Ref(null, 'component', 'component/solid/1', 'stable-topology')
    const target = topology('stable-topology')
    assert.equal(ref.update(tree([solid('component/solid/1', [target])])), undefined)
    for(const model of [tree([]), { findChild: () => undefined }, tree([
      solid('component/solid/0', [target]),
      solid('component/solid/2', [topology('stable-topology')]),
    ])]) {
      assert.equal(ref.update(model)?.type, 'error')
      assert.equal(ref.getItem(), null)
    }
  })
}
