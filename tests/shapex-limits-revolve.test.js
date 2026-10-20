import assert from 'node:assert/strict'
import { tessellateMesh } from '../js/core/shapex.js'
import { test, construction, fixtureFace, revolveLimitsFixture } from './helpers/revolve-limits.mjs'

test('document 9: revolve circle to both spline surfaces in both directions', () => {
  const { body, source, axis } = revolveLimitsFixture()
  const full = source.revolve('c', axis, 2 * Math.PI, 'full')
  assert.ok(full.volume() > 0)
  const targets = body.faces().filter(face => !face.getPlane())
  assert.equal(targets.length, 3)
  const errors = []
  const upper = fixtureFace(body, '/extrude-id-3826/swept/id-20296')
  const lower = fixtureFace(body, '/extrude-id-42724/swept/id-42703')
  for(const [from, until] of [[upper, lower], [lower, upper]]) {
    for(const forward of [true, false]) {
      try {
        const result = source.revolveBetweenFaces('c', axis, from, until, 'between', forward)
        assert.equal(result.solids().length, 1)
        assert.ok(result.volume() > 0 && result.volume() < full.volume())
        tessellateMesh(result.geom())
        for(const operation of ['join', 'cut', 'intersect']) {
          try {
            const output = body.boolean(result, operation)
            tessellateMesh(output.geom())
          } catch(error) { errors.push({ from: from.id, until: until.id, forward, operation, error }) }
        }
      } catch(error) { errors.push({ from: from.id, until: until.id, forward, error }) }
    }
  }
  for(const target of targets) {
    for(const forward of [true, false]) {
      // The opposite wall face ends inside the torus at the wall's base.
      // It intersects the torus, but cannot cap the entire swept circle.
      if(construction(target, 'id-3826', 'profile-side', 'id-3771')) {
        assert.throws(() => source.revolveUpToFace('c', axis, target, 'limit', forward),
          error => error.message.includes('must cap the entire profile'))
        continue
      }
      try {
        const result = source.revolveUpToFace('c', axis, target, 'limit', forward)
        assert.equal(result.solids().length, 1)
        assert.ok(result.volume() > 0 && result.volume() < full.volume())
        tessellateMesh(result.geom())
        for(const operation of ['join', 'cut', 'intersect']) {
          try {
            const output = body.boolean(result, operation)
            tessellateMesh(output.geom())
          }
          catch(error) { errors.push({ face: target.id, forward, operation, error }) }
        }
      } catch(error) { errors.push({ face: target.id, forward, error }) }
    }
  }
  assert.deepEqual(errors, [])
})
