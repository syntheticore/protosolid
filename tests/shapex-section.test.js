import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { initialize, loadDocument } from './shapex-cad/runtime.mjs'
import { CreateSketchFeature } from '../js/core/features.js'
import { PlanarReference } from '../js/core/references.js'
import Serialize from '../js/core/serialize.js'

await initialize()
const source = await readFile(new URL('./fixtures/rounded-shell-top-section.cad', import.meta.url), 'utf8')

test('Shapex: sketch sections on a rounded shell cap retain shared curved contact vertices after reload', () => {
  const document = loadDocument(source)
  const compound = document.top().compound
  const top = compound.faces().find(face => {
    const plane = face.getPlane()
    return plane && Math.abs(plane.elements[13] - 63) < 1e-7 && Math.abs(plane.elements[5]) < 1e-7
  })
  assert.ok(top)
  const sketch = new CreateSketchFeature(document)
  const reference = new PlanarReference(top.faceReference())
  sketch.plane = () => reference
  document.timeline.insertFeature(sketch, false)
  document.timeline.regenerate(9, 10)

  const check = doc => {
    for(const feature of doc.timeline.features) assert.equal(feature.error, undefined)
    const body = doc.top().compound
    const plane = doc.timeline.features.at(-1).sketch.workplane
    const below = body.halfCut(plane, true)
    const above = body.halfCut(plane, false)
    assert.equal(below.solids().length, 1)
    assert.equal(above.solids().length, 0)
    assert.equal(below.faces().length, body.faces().length, 'a tangent section must preserve every face')
    const originalVolume = body.solids()[0].geom().volume()
    assert.ok(Math.abs(below.solids()[0].geom().volume() - originalVolume) < originalVolume * 1e-8)
    for(const face of below.faces()) assert.ok(face.tesselate().positions.length)
  }
  check(document)
  document.timeline.regenerate(0, 10)
  check(document)
  check(loadDocument(Serialize.stringify({ timeline: document.timeline })))
})
