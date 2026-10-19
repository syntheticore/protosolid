import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import * as THREE from 'three'
import { initializeShapex, shapex as sx } from '../js/core/shapex.js'
import { Arc, Circle, Line, Spline, DerivedCurve } from '../js/core/geom2d.js'
import { buildOffsetCurves } from '../js/core/sketch-offset.js'
import { Sketch, Projection, ProfileOffsetConstraint } from '../js/core/sketch.js'
import { CircleTool } from '../js/tools.js'
import Snapper from '../js/snapping.js'
import SketchPlane from '../js/three/sketch-plane.js'

await initializeShapex({ wasm: await readFile(new URL('../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)) })
globalThis.window = { addEventListener() {} }
const vector = (x, y) => new THREE.Vector3(x, y, 0)
const near = (a, b) => assert.ok(a.distanceTo(b) < 1e-7, `${a.toArray()} != ${b.toArray()}`)

test('circle previews can shrink to the center without creating kernel geometry', () => {
  const sketch = new Sketch()
  const circle = new Circle(vector(0, 0), 0)
  sketch.add(circle)
  assert.equal(circle.geom, null)
  assert.deepEqual(sketch.profileElements(), [])
  circle.setHandles([vector(0, 0), vector(2, 0)])
  assert.ok(circle.geom)
  circle.setHandles([vector(0, 0), vector(0, 0)])
  assert.equal(circle.geom, null)
  assert.equal(circle.length(), 0)
  assert.deepEqual(SketchPlane.prototype.sampleSketch([circle]).map(point => point.toArray()), [[0, 0, 0]])
  assert.throws(() => sx.TrimmedCurve2.circle(sx.Point2.new(0, 0), 0), RangeError)
})

test('a second center click keeps a circle pending until it has a radius', () => {
  const sketch = new Sketch()
  const tool = new CircleTool(null, { elementChanged() {}, snapper: { snapped: {} } }, sketch)
  tool.mouseDown(vector(0, 0), null)
  const pending = tool.curve
  assert.equal(pending.geom, null)
  tool.mouseMove(vector(2, 0))
  assert.equal(pending.radius, 2)
  tool.mouseMove(vector(0, 0))
  tool.mouseDown(vector(0, 0), null)
  assert.equal(tool.curve, pending)
  assert.ok(tool.center)
  assert.deepEqual(sketch.profileElements(), [])
  tool.mouseDown(vector(3, 0), null)
  assert.equal(tool.curve, null)
  assert.equal(tool.center, null)
  assert.equal(pending.radius, 3)
  assert.deepEqual(sketch.profileElements(), [pending])
})

test('canceling a zero-radius circle removes its preview', () => {
  const sketch = new Sketch()
  const tool = new CircleTool(null, { elementChanged() {}, snapper: { snapped: {} } }, sketch)
  tool.mouseDown(vector(0, 0), null)
  tool.mouseMove(vector(0, 0))
  tool.dispose()
  assert.deepEqual(sketch.elements, [])
})

test('the sketch grid footprint samples native curve tessellation', () => {
  const circle = new Circle(vector(0, 0), 3)
  const points = SketchPlane.prototype.sampleSketch([circle])
  assert.equal(points.length, 16)
  assert.ok(points.every(point => Number.isFinite(point.x) && Number.isFinite(point.y)))
  assert.ok(Math.min(...points.map(point => point.x)) < -2.8)
  assert.ok(Math.max(...points.map(point => point.y)) > 2.8)
  const line = new Line(vector(0, 0), vector(2, 0))
  assert.deepEqual(SketchPlane.prototype.sampleSketch([line]).map(point => point.toArray()), [[0, 0], [2, 0]])
})

test('trimmed sketch splines retain their boundary through cloning and reversal', () => {
  const source = new Spline([vector(0, 0), vector(2, 3), vector(4, 3), vector(6, 0)])
  const trim = source.geom().trimmed(sx.ParameterRange.new(.25, .75))
  const piece = Spline.fromGeometry(trim, 'piece')
  near(piece.endpoints()[0], source.sample(.25))
  near(piece.endpoints()[1], source.sample(.75))
  const clone = piece.clone()
  clone.flip()
  near(clone.endpoints()[0], source.sample(.75))
  near(clone.endpoints()[1], source.sample(.25))
  near(piece.endpoints()[0], source.sample(.25))
})

test('saved clockwise arcs and solved counter-clockwise arcs preserve traversal', () => {
  const clockwise = Arc.undump({ id: 'arc', _center: vector(0, 0), radius: 2, start: 0, sweep: -Math.PI / 2 })
  near(clockwise.sample(0), vector(2, 0))
  near(clockwise.sample(1), vector(0, -2))
  const counter = Arc.fromPoints([vector(2, 0), vector(Math.sqrt(2), Math.sqrt(2)), vector(0, 2)])
  counter.setSolvedHandles([vector(1, 1), vector(4, 1), vector(1, 4)], 3)
  const restored = Arc.undump(counter.dump())
  near(restored.sample(0), vector(4, 1))
  near(restored.sample(1), vector(1, 4))
  near(restored.sample(.5), vector(1 + 3 / Math.sqrt(2), 1 + 3 / Math.sqrt(2)))
})

test('derived sketch offsets clone independently and preserve their signed side', () => {
  const source = new Line(vector(0, 0), vector(10, 0))
  const [offset] = buildOffsetCurves([[{ curve: source, start: 0, end: 1 }]], 2)
  assert.ok(offset instanceof DerivedCurve)
  near(offset.sample(0), vector(0, 2))
  near(offset.sample(1), vector(10, 2))
  const clone = offset.clone()
  clone.flip()
  near(clone.sample(0), vector(10, 2))
  near(offset.sample(0), vector(0, 2))
})

test('rectangles joined by a bridge line each form one profile', () => {
  // The outer face walk retraces the bridge, so it is not a simple wire.
  const sketch = new Sketch()
  const line = (x0, y0, x1, y1) => {
    const curve = new Line(vector(x0, y0), vector(x1, y1))
    sketch.add(curve)
    return curve
  }
  const rectangle = (x0, y0, x1, y1) => [line(x0, y0, x1, y0), line(x1, y0, x1, y1), line(x1, y1, x0, y1), line(x0, y1, x0, y0)]
  // One rectangle below the bridge line on the right, one above it on the left.
  rectangle(20, -10, 30, 0)
  const bridge = line(20, 0, 10, 0)
  rectangle(0, 0, 10, 10)
  const profiles = sketch.profiles(null, false)
  assert.equal(profiles.length, 2)
  for(const profile of profiles) {
    assert.equal(profile.rings.length, 1)
    assert.ok(profile.rings[0].segments.every(segment => segment.id != bridge.id))
    assert.ok(Math.abs(profile.geom().area() - 100) < 1e-9)
  }
})

test('hovering near a spline produces a curve snap', () => {
  const spline = new Spline([vector(0, 0), vector(2, 3), vector(4, 3), vector(6, 0)])
  const snapper = new Snapper({ renderer: { toScreen: point => new THREE.Vector2(point.x, point.y) } }, () => {})
  snapper.getActiveHandleConnections = () => new Set()
  snapper.getSnapElements = () => [spline]
  snapper.getSnapPointTargets = () => []
  snapper.activeHandleReachability = () => () => true
  const target = snapper.catchSnapTargets(vector(3, 3), new THREE.Vector2(3, 3), false)
  assert.equal(target.elem, spline)
  assert.ok(spline.distanceTo(target.point) < 1e-7)
})

test('invalid arc handles leave the accepted geometry unchanged', () => {
  const arc = Arc.fromPoints([vector(2, 0), vector(Math.sqrt(2), Math.sqrt(2)), vector(0, 2)])
  const geometry = arc.geom(), before = arc.dump()
  for(const radius of [0, 1e-8, NaN, Infinity]) {
    assert.equal(arc.setSolvedHandles([vector(0, 0), vector(2, 0), vector(0, 2)], radius), false)
  }
  for(const value of [NaN, Infinity]) {
    assert.equal(arc.setSolvedHandles([vector(value, 0), vector(2, 0), vector(0, 2)], 2), false)
  }
  assert.equal(arc.setSolvedHandles([vector(0, 0), vector(2, 0), vector(2, 0)], 2), false)
  assert.equal(arc.geom(), geometry)
  assert.deepEqual(arc.dump(), before)
})

test('circle splitting merges intersections on either side of the seam', () => {
  const circle = new Circle(vector(0, 0), 1)
  circle.splitIntersections = () => [vector(1, 0), vector(1, 1e-9), vector(-1, 0)]
  const pieces = circle.split([])
  assert.equal(pieces.length, 2)
  assert.ok(pieces.every(piece => Math.abs(piece.geom().range().span()) > 3))
})

test('degenerate projection input invalidates the projection', () => {
  const spatial = sx.TrimmedCurve3.new(
    sx.Line3.through(sx.Point3.new(0, 0, 0), sx.Point3.new(1, 0, 0)), sx.ParameterRange.new(0, 1),
  )
  const projection = new Projection({ update() {}, getItem: () => ({ geom: () => spatial }) })
  projection.sketch = { creator: {}, workplane: new THREE.Matrix4().makeScale(0, 1, 1) }
  const previous = new Line(vector(0, 0), vector(1, 0))
  projection.output = previous
  projection.update({ findChild() {} })
  assert.equal(projection.output, undefined)
  assert.equal(previous.geom, null)
  assert.match(projection.error, /cannot be projected/)
})

const chain = curves => curves.map(curve => ({ curve, start: 0, end: 1 }))
const rectangle = (width, height) => {
  const points = [vector(0, 0), vector(width, 0), vector(width, height), vector(0, height)]
  return chain(points.map((point, index) => new Line(point, points[(index + 1) % 4])))
}

test('sketch offsets reject collapse, crossings and disconnected sources', () => {
  const box = rectangle(10, 4)
  assert.equal(buildOffsetCurves([box], 1).length, 4)
  assert.throws(() => buildOffsetCurves([box], 2))
  assert.throws(() => buildOffsetCurves([box], 3))
  const horizontal = chain([new Line(vector(-2, 0), vector(2, 0))])
  const vertical = chain([new Line(vector(0, -2), vector(0, 2))])
  assert.throws(() => buildOffsetCurves([horizontal, vertical], .5))
  const disconnected = chain([new Line(vector(0, 0), vector(1, 0)), new Line(vector(2, 0), vector(2, 1))])
  assert.throws(() => buildOffsetCurves([disconnected], .1))
  const crossing = new Spline([vector(0, 0), vector(3, 3), vector(-2, 3), vector(1, 0)])
  assert.throws(() => buildOffsetCurves([chain([crossing])], .01))
})

test('mixed sketch boundaries retain line miters and sharp corners remain uncapped', () => {
  const arc = Arc.fromPoints([vector(10, 3), vector(9 + Math.SQRT1_2, 3 + Math.SQRT1_2), vector(9, 4)])
  const mixed = chain([
    new Line(vector(0, 0), vector(10, 0)), new Line(vector(10, 0), vector(10, 3)), arc,
    new Line(vector(9, 4), vector(0, 4)), new Line(vector(0, 4), vector(0, 0)),
  ])
  const outside = buildOffsetCurves([mixed], -1)
  assert.equal(outside.length, 5)
  near(outside[0].sample(0), vector(-1, -1))
  near(outside[0].sample(1), vector(11, -1))
  const sharp = chain([new Line(vector(0, 0), vector(10, 0)), new Line(vector(10, 0), vector(0, 1))])
  assert.equal(buildOffsetCurves([sharp], -1).length, 2)
})

test('a rejected offset restores its accepted distance and reports the failure', () => {
  const groups = [rectangle(10, 4)]
  const outputs = buildOffsetCurves(groups, 1)
  const sketch = { id: 'sketch', component: { id: 'component' } }
  for(const curve of [...groups[0].map(entry => entry.curve), ...outputs]) curve.sketch = sketch
  const constraint = new ProfileOffsetConstraint(groups, outputs, vector(1, 1), 1)
  constraint.sketch = sketch
  const previous = outputs.map(curve => curve.sample(.5))
  const messages = [], bus = window.bus
  window.bus = { emit: (...args) => messages.push(args) }
  try {
    constraint.distance = 3
    constraint.expression = '3'
    assert.equal(constraint.rebuild(), false)
    assert.equal(constraint.distance, 1)
    assert.equal(constraint.expression, null)
    outputs.forEach((curve, index) => near(curve.sample(.5), previous[index]))
    assert.equal(messages[0][0], 'toast')
  } finally { window.bus = bus }
})
