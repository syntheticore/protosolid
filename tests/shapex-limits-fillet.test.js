import assert from 'node:assert/strict'
import { shapex, tessellateMesh } from '../js/core/shapex.js'
import { test, construction, fixtureFace, revolutionEdge, revolveLimitsFixture } from './helpers/revolve-limits.mjs'

test('document 9: fillet joined revolution and bounded spline edges', () => {
  const { body, source, axis } = revolveLimitsFixture()
  const upper = fixtureFace(body, '/extrude-id-3826/swept/id-20296')
  const lower = fixtureFace(body, '/extrude-id-42724/swept/id-42703')
  const tool = source.revolveBetweenFaces('c', axis, upper, lower, 'id-67306', true)
  const joined = body.boolean(tool, 'join')
  const edges = joined.solids().flatMap(solid => solid.edges()).filter(edge =>
    edge.connectedFaces().length == 2)
  // These two curved rims need continuation past the spline's natural end;
  // that endpoint-extension case is separate from the bounded-guide fix.
  const unsupportedRims = new Set(edges.filter(edge => {
    const faces = edge.connectedFaces()
    return faces.some(face => construction(face, 'id-42724', 'profile-side', 'id-42703')) &&
      faces.some(face => construction(face, 'id-42724', 'profile-start') || construction(face, 'id-42724', 'profile-end'))
  }).map(edge => edge.id))
  assert.equal(edges.filter(edge => unsupportedRims.has(edge.id)).length, 2)
  const rims = edges.filter(edge => revolutionEdge(edge))
  assert.equal(rims.length, 2)
  for(const edge of rims) {
    const endpoints = edge.vertices()
    assert.ok(endpoints[0].distanceTo(endpoints[1]) < 1e-7)
    const selection = edge.edgeReference().selection
    assert.equal(selection.policy, 'single')
    assert.equal(joined.resolveTopology(selection).status, 'resolved')
  }
  const lowerRim = edges.filter(edge => revolutionEdge(edge)
    && edge.connectedFaces().some(face => face.id == lower.id))
  assert.equal(lowerRim.length, 1)
  const candidates = edges.filter(edge => !unsupportedRims.has(edge.id))
  const splineEdge = candidates.find(edge => !revolutionEdge(edge)
    && edge.geom().curve() instanceof shapex.TrimmedCurve3
    && edge.connectedFaces().some(face => construction(face, 'id-3826', 'profile-side', 'id-20296')))
  assert.ok(splineEdge, 'retain a bounded spline edge in the expanded radius checks')
  const representatives = new Set([...rims, splineEdge])
  const exhaustive = process.env.PROTOSOLID_TEST_EXHAUSTIVE == '1'
  const errors = []
  // Every edge gets the standard radius. The two revolution rims and a bounded
  // spline edge cover all radii; the exhaustive command restores the full matrix.
  for(const edge of candidates) {
    const radii = exhaustive || representatives.has(edge) ? [0.25, 0.5, 1, 2, 5] : [1]
    for(const radius of radii) {
      try {
        const result = joined.fillet([edge], radius)
        assert.ok(result.volume() > 0)
        tessellateMesh(result.geom())
      } catch(error) { errors.push({ edge: edge.id, radius, error }) }
    }
  }
  assert.deepEqual(errors, [])
})
