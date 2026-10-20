import assert from 'node:assert/strict'
import * as THREE from 'three'
import { Profile, Wire } from '../js/core/geom3d.js'
import { Line, Circle } from '../js/core/geom2d.js'
import { rotationFromNormal } from '../js/core/utils.js'
import { test, construction } from './helpers/revolve-limits.mjs'

function rectangle(x0, y0, x1, y1, workplane = new THREE.Matrix4()) {
  const corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(([x, y]) => new THREE.Vector3(x, y, 0))
  const segments = corners.map((point, index) => {
    const segment = new Line(point, corners[(index + 1) % 4])
    segment.id = 'edge-' + index
    return segment
  })
  return new Profile({ workplane }, [new Wire(segments)])
}

function cap(body, role) {
  return body.faces().find(face => body.geom().creationRole(face.geom())?.kind ==
    (role == 'top' ? 'extrusion-end' : 'extrusion-start'))
}

function circle(x, y, radius, workplane = new THREE.Matrix4()) {
  const segment = new Circle(new THREE.Vector3(x, y, 0), radius)
  segment.id = 'circle'
  return new Profile({ workplane }, [new Wire([segment])])
}

function cylinderLimit(angle) {
  const body = circle(4 * Math.cos(angle), 4 * Math.sin(angle), 3,
    new THREE.Matrix4().makeTranslation(0, 0, -2)).extrude('c', 4, 'target')
  return body.faces().find(face => !face.getPlane())
}

test('section hatching preserves the planar face around a circular boss', () => {
  const base = rectangle(0, 0, 10, 10).extrude('c', 5, 'base')
  const boss = circle(5, 5, 2).extrude('c', 8, 'boss')
  const body = base.boolean(boss, 'join')
  const plane = new THREE.Matrix4().makeTranslation(0, 0, 5)
  const cut = body.halfCut(plane, true)
  const originalTop = body.faces().find(face => {
    const p = face.getPlane()
    return p && Math.abs(p.elements[14] - 5) < 1e-7
  })
  const unchanged = cut.preservedFaces(body.faces())
  assert.ok([...unchanged].some(face => face.id == originalTop.id))
  const sectionFaces = cut.faces().filter(face => {
    const p = face.getPlane()
    return p && Math.abs(p.elements[14] - 5) < 1e-7 && !unchanged.has(face)
  })
  assert.equal(sectionFaces.length, 1)
})

function near(actual, expected) {
  assert.ok(Math.abs(actual - expected) < 1e-5 * Math.max(1, Math.abs(expected)), `${actual} != ${expected}`)
}

test('circle on a cylinder cap extrudes back to the opposite planar face', () => {
  const body = circle(2.6559596628004023, 5.6499505320061365, 41.458176985310196,
    rotationFromNormal(new THREE.Vector3(1, 0, 0))).extrude('c', 38, 'body')
  const source = circle(10.202437506387206, 7.213174143629522, 20.834734325948986,
    cap(body, 'top').getPlane())
  const result = source.extrudeUpToFace('c', cap(body, 'bottom'), 'limit')
  near(result.volume(), Math.PI * 20.834734325948986 ** 2 * 38)
  near(body.boolean(result, 'cut').volume(), body.volume() - result.volume())
})

test('extrusion limits use the infinite plane, support offsets and reverse direction', () => {
  const source = rectangle(-1, -1, 1, 1)
  const target = rectangle(20, 20, 21, 21).extrude('c', 10, 'target')
  const result = source.extrudeUpToFace('c', cap(target, 'top'), 'limit')
  near(result.volume(), 40)
  assert.ok(cap(result, 'bottom'))
  assert.ok(cap(result, 'top'))
  assert.equal(result.faces().filter(face => construction(face, 'limit', 'profile-side')).length, 4)
  near(source.extrudeUpToFace('c', cap(target, 'top'), 'limit', 2).volume(), 32)
  const below = rectangle(20, 20, 21, 21).extrude('c', -10, 'below')
  near(source.extrudeUpToFace('c', cap(below, 'top'), 'limit', 2).volume(), 32)
  assert.throws(() => source.extrudeUpToFace('c', cap(target, 'top'), 'limit', 11))
})

test('oblique extrusion limit keeps the complete start and exact sloped cap', () => {
  const source = rectangle(-1, -1, 1, 1)
  const plane = new THREE.Matrix4().makeRotationY(Math.PI / 6).setPosition(0, 0, 10)
  const target = rectangle(-4, -4, 4, 4, plane).extrude('c', 1, 'target')
  near(source.extrudeUpToFace('c', cap(target, 'bottom'), 'limit').volume(), 40)
  const crossing = rectangle(-4, -4, 4, 4, new THREE.Matrix4().makeRotationY(Math.PI / 6)).extrude('c', 1, 'cross')
  assert.throws(() => source.extrudeUpToFace('c', cap(crossing, 'bottom'), 'limit'))
})

const axis = new THREE.Matrix4()
const profilePlane = new THREE.Matrix4().makeRotationX(Math.PI / 2)
function radialLimit(angle, id) {
  const plane = new THREE.Matrix4().makeRotationZ(angle).multiply(profilePlane)
  return cap(rectangle(1, -2, 4, 2, plane).extrude('c', 1, id), 'bottom')
}

test('revolve limits choose the first forward or reverse radial plane crossing', () => {
  const source = rectangle(2, -1, 3, 1, profilePlane)
  near(source.revolveUpToFace('c', axis, radialLimit(Math.PI / 3, 'a'), 'limit', true).volume(), 5 * Math.PI / 3)
  near(source.revolveUpToFace('c', axis, radialLimit(-Math.PI / 3, 'b'), 'limit', false).volume(), 5 * Math.PI / 3)
})

test('revolve between planes supports an interval wrapping past the sketch', () => {
  const source = rectangle(2, -1, 3, 1, profilePlane)
  near(source.revolveBetweenFaces('c', axis, radialLimit(Math.PI / 6, 'a'), radialLimit(Math.PI / 3, 'b'), 'limit').volume(), 5 * Math.PI / 6)
  near(source.revolveBetweenFaces('c', axis, radialLimit(Math.PI / 3, 'a'), radialLimit(Math.PI / 6, 'b'), 'limit').volume(), 5 * 11 * Math.PI / 6)
})

test('extrusion to a curved face chooses the first cap and rejects insufficient coverage', () => {
  const plane = new THREE.Matrix4().makeRotationX(Math.PI / 2).setPosition(0, 10, 10)
  const body = circle(0, 0, 5, plane).extrude('c', 20, 'cylinder')
  const target = body.faces().find(face => !face.getPlane())
  const source = rectangle(-1, -1, 1, 1)
  const result = source.extrudeUpToFace('c', target, 'limit')
  near(result.volume(), 40 - 2 * (Math.sqrt(24) + 25 * Math.asin(0.2)))
  assert.throws(() => rectangle(-6, -1, 6, 1).extrudeUpToFace('c', target, 'limit'))
})

test('revolve to and between curved faces', () => {
  const source = rectangle(2, -1, 3, 1, profilePlane)
  const target = cylinderLimit(Math.PI / 2)
  const result = source.revolveUpToFace('c', axis, target, 'limit')
  let expected = 0
  const count = 10000
  for(let i = 0; i < count; i++) {
    const r = 2 + (i + 0.5) / count
    expected += 2 * r * Math.asin((r * r + 7) / (8 * r)) / count
  }
  near(result.volume(), expected)
  const from = cylinderLimit(Math.PI / 3)
  const until = cylinderLimit(5 * Math.PI / 9)
  const between = source.revolveBetweenFaces('c', axis, from, until, 'between')
  const wrapped = source.revolveBetweenFaces('c', axis, until, from, 'wrap')
  near(between.volume(), 5 * 2 * Math.PI / 9)
  near(wrapped.volume(), 5 * 16 * Math.PI / 9)
  for(const [body, owner] of [[result, 'limit'], [between, 'between'], [wrapped, 'wrap']]) {
    assert.equal(body.faces().filter(face => construction(face, owner, 'profile-start')).length, 1)
    assert.equal(body.faces().filter(face => construction(face, owner, 'profile-end')).length, 1)
    for(const item of [...body.faces(), ...body.edges().filter(edge => edge.isSemantic())]) {
      assert.equal(body.resolveTopology(body.topologySelection(item)).status, 'resolved')
    }
  }
})

test('non-radial planes and late curved limits span quarter-turn boundaries', () => {
  const source = rectangle(2, -1, 3, 1, profilePlane)
  const target = cap(rectangle(-5, -5, 5, 5,
    new THREE.Matrix4().makeRotationY(Math.PI / 2).setPosition(-1, 0, 0)).extrude('c', 1, 'plane'), 'bottom')
  let expected = 0
  for(let i = 0; i < 10000; i++) {
    const r = 2 + (i + 0.5) / 10000
    expected += 2 * r * Math.acos(-1 / r) / 10000
  }
  near(source.revolveUpToFace('c', axis, target, 'late').volume(), expected)
  const curved = cylinderLimit(7 * Math.PI / 6)
  const early = cylinderLimit(Math.PI / 2)
  near(source.revolveUpToFace('c', axis, curved, 'late').volume(),
    source.revolveUpToFace('c', axis, early, 'early').volume() + 5 * 2 * Math.PI / 3)
  const oblique = cap(rectangle(-5, -5, 5, 5,
    new THREE.Matrix4().makeRotationY(Math.PI / 3)).extrude('c', 1, 'oblique'), 'bottom')
  near(source.revolveUpToFace('c', axis, oblique, 'oblique').volume(), 5 * Math.PI / 2)
})

test('two-sided extrusion, angular offsets, holes and invalid limit selection', () => {
  const source = rectangle(-2, -2, 2, 2)
  source.rings.push(rectangle(-1, -1, 1, 1).rings[0])
  near(source.extrude('c', 20, 'both', -10).volume(), 240)
  const target = cap(rectangle(10, 10, 11, 11).extrude('c', 10, 'target'), 'top')
  near(source.extrudeUpToFace('c', target, 'hole').volume(), 120)
  const radial = rectangle(2, -1, 3, 1, profilePlane)
  near(radial.revolve('c', axis, Math.PI / 3, 'offset', Math.PI / 4).volume(), 5 * Math.PI / 3)
  assert.throws(() => radial.revolveBetweenFaces('c', axis, target, target, 'invalid'))
  assert.throws(() => radial.revolveUpToFace('c', axis, radialLimit(0, 'start'), 'invalid'))
})

test('document 8: cutting with a circle ending on the cylinder leaves the cylinder intact', () => {
  const cylinderProfile = circle(9.609744834740159, 11.348191136502194, 26.23894553307264,
    rotationFromNormal(new THREE.Vector3(1, 0, 0)))
  const body = cylinderProfile.extrude('c', 82, 'cylinder', -41)
  const source = circle(-1.3457094482737189, 12.454480426114497, 14.912577936970921,
    rotationFromNormal(new THREE.Vector3(0, 1, 0)).setPosition(0, -49, 0))
  const target = body.faces().find(face => !face.getPlane())
  const tool = source.extrudeUpToFace('c', target, 'limit')
  near(body.boolean(tool, 'join').volume(), body.volume() + tool.volume())
  near(body.boolean(tool, 'cut').volume(), body.volume())
  near(body.boolean(tool, 'join').volume(), body.volume() + tool.volume())
})
