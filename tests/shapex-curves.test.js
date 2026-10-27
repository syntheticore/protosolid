import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import initShapex from '../js/shapexJS/index.js'

const sx = await initShapex({ wasm: await readFile(new URL('../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)) })
const p = (x, y) => sx.Point2.new(x, y)
const near = (actual, expected, tolerance = 1e-7) => assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} != ${expected}`)
const pointNear = (actual, expected) => { near(actual.x, expected.x); near(actual.y, expected.y) }

test('basis queries are unbounded; trimmed queries include endpoints', () => {
  const curve = sx.TrimmedCurve2.line(p(0, 0), p(10, 0))
  near(curve.curve().closestPoint(p(20, 2)).parameter, 2)
  const bounded = curve.closestPoint(p(20, 2))
  near(bounded.parameter, 1)
  pointNear(bounded.point, p(10, 0))
  near(curve.length(), 10)
  assert.throws(() => curve.evaluate(2), RangeError)
})

test('splits and reversal preserve source parameters and traversal', () => {
  const curve = sx.TrimmedCurve2.line(p(0, 0), p(10, 0)).reversed()
  const pieces = curve.splitAt([0.25, 0.75, 0.25])
  assert.deepEqual(pieces.map(piece => [piece.range().start, piece.range().end]), [[1, .75], [.75, .25], [.25, 0]])
  pointNear(pieces[0].start(), p(10, 0))
  pointNear(pieces.at(-1).end(), p(0, 0))
})

test('intersections distinguish tangency, crossings and periodic overlap', () => {
  const circle = sx.TrimmedCurve2.circle(p(0, 0), 2)
  const tangent = sx.TrimmedCurve2.line(p(-3, 2), p(3, 2)).intersections(circle)
  assert.equal(tangent.length, 1)
  assert.equal(tangent[0].contact, 'tangent')
  const crossing = sx.TrimmedCurve2.line(p(-3, 0), p(3, 0)).intersections(circle)
  assert.equal(crossing.length, 2)
  assert.ok(crossing.every(hit => hit.contact == 'crossing'))
  const overlap = circle.intersections(circle.reversed())
  assert.equal(overlap.length, 1)
  assert.equal(overlap[0].kind, 'overlap')
  near(Math.abs(overlap[0].first.span()), Math.PI * 2)
  const first = circle.trimmed(sx.ParameterRange.new(0, Math.PI / 2))
  const second = circle.trimmed(sx.ParameterRange.new(Math.PI / 2, Math.PI))
  const contact = first.intersections(second)
  assert.equal(contact.length, 1)
  pointNear(contact[0].point, p(0, 2))
})

test('spline self-intersections retain both distinct source parameters', () => {
  const basis = sx.NurbsCurve2.new(3, [p(0, 0), p(3, 3), p(-2, 3), p(1, 0)], [1, 1, 1, 1], [0, 0, 0, 0, 1, 1, 1, 1])
  const hits = basis.trimmed(sx.ParameterRange.new(0, 1)).selfIntersections()
  assert.equal(hits.length, 1)
  pointNear(hits[0].point, p(.5, .5625))
  assert.ok(Math.abs(hits[0].firstParameter - hits[0].secondParameter) > .8)
})

test('spline offsets remain exact derived curves and reverse their signed side', () => {
  const basis = sx.NurbsCurve2.new(3, [p(0, 0), p(2, 1), p(4, 1), p(6, 0)], [1, 1, 1, 1], [0, 0, 0, 0, 1, 1, 1, 1])
  const source = basis.trimmed(sx.ParameterRange.new(0, 1))
  const offset = source.offset(.3)
  assert.ok(offset.curve() instanceof sx.Curve2)
  for(const u of [0, .2, .5, .8, 1]) {
    const a = source.evaluate(u), b = offset.evaluate(u)
    const speed = a.firstDerivative.magnitude()
    pointNear(b.point, p(a.point.x - .3 * a.firstDerivative.y / speed, a.point.y + .3 * a.firstDerivative.x / speed))
    pointNear(source.reversed().offset(-.3).evaluate(u).point, b.point)
  }
  const tolerance = sx.ModelTolerance.new(1e-5, 1e-12, 1e-10, 1e-12)
  const approximation = offset.approximateNurbs(tolerance)
  assert.ok(approximation.curve.curve() instanceof sx.NurbsCurve2)
  assert.ok(approximation.estimatedError <= tolerance.linear)
  for(let i = 0; i <= 100; i++) {
    const a = offset.evaluate(i / 100).point, b = approximation.curve.evaluate(i / 100).point
    assert.ok(Math.hypot(a.x - b.x, a.y - b.y) <= tolerance.linear * 1.1)
  }
  const wire = sx.Wire2.new([offset, sx.TrimmedCurve2.line(offset.end(), offset.start())])
  const profile = sx.Profile.new(wire)
  const body = profile.extrude(sx.Plane3.xy(), sx.Vec3.new(0, 0, 2))
  near(body.volume(), profile.area() * 2, 1e-5)
  const turn = profile.revolve(sx.Plane3.xy(), sx.Axis3.new(sx.Point3.new(-10, 0, 0), sx.Vec3.new(0, 1, 0)), Math.PI / 4)
  assert.ok(turn.volume() > 0)
  const rational = profile.approximateRational(tolerance)
  assert.ok(rational.estimatedError <= tolerance.linear)
  assert.ok(rational.profile.outer().segments().every(segment =>
    segment.curve() instanceof sx.Line2 || segment.curve() instanceof sx.NurbsCurve2))
})

test('wire offsets expose joins and source correspondence', () => {
  const wire = sx.Wire2.new([sx.TrimmedCurve2.line(p(0, 0), p(10, 0)), sx.TrimmedCurve2.line(p(10, 0), p(10, 10))])
  const result = wire.offset(-1)
  assert.equal(result.wire.segments().length, 3)
  assert.deepEqual(result.sources, [0, null, 1])
  pointNear(result.wire.segments()[0].end(), result.wire.segments()[1].start())
  pointNear(result.wire.segments()[1].end(), result.wire.segments()[2].start())
  const square = sx.Wire2.polygon([p(0, 0), p(10, 0), p(10, 10), p(0, 10)])
  const miter = sx.Wire2.new(square.segments()).offset(-1, { join: 'miter' })
  near(sx.Wire2.new(miter.wire.segments()).area(), 144)
})

test('wire simplicity is explicit and rejects crossings and overlaps', () => {
  const line = (a, b) => sx.TrimmedCurve2.line(p(...a), p(...b))
  const simple = sx.Wire2.new([line([0, 0], [1, 0]), line([1, 0], [1, 1])])
  simple.validateSimple()
  sx.Wire2.new([sx.TrimmedCurve2.circle(p(0, 0), 1)]).validateSimple()
  const crossing = sx.Wire2.new([
    line([0, 0], [2, 2]), line([2, 2], [0, 2]), line([0, 2], [2, 0]),
  ])
  assert.throws(() => crossing.validateSimple(), sx.ShapexError)
  const overlap = sx.Wire2.new([line([0, 0], [2, 0]), line([2, 0], [1, 0])])
  assert.throws(() => overlap.validateSimple(), sx.ShapexError)
})

test('wire offsets support per-corner styles and explicit uncapped miters', () => {
  const points = [[0, 0], [10, 0], [10, 10], [0, 10]].map(([x, y]) => p(x, y))
  const wire = sx.Wire2.new(points.slice(0, -1).map((point, index) => sx.TrimmedCurve2.line(point, points[index + 1])))
  const mixed = wire.offset(-1, { join: ['round', 'miter'] })
  assert.deepEqual(mixed.sources, [0, null, 1, 2])
  mixed.wire.validateSimple()
  assert.throws(() => wire.offset(-1, { join: ['miter'] }), sx.ShapexError)
  const sharp = sx.Wire2.new([
    sx.TrimmedCurve2.line(p(0, 0), p(10, 0)), sx.TrimmedCurve2.line(p(10, 0), p(0, 1)),
  ])
  assert.equal(sharp.offset(-1, { join: 'miter' }).wire.segments().length, 3)
  assert.equal(sharp.offset(-1, { join: 'miter', miterLimit: Infinity }).wire.segments().length, 2)
  const box = sx.Wire2.polygon([p(0, 0), p(10, 0), p(10, 4), p(0, 4)])
  assert.throws(() => box.offset(3, { join: 'miter' }), sx.ShapexError)
})

test('region offsets expand, erode and remove collapsed boundaries', () => {
  const square = sx.Profile.new(sx.Wire2.polygon([p(0, 0), p(10, 0), p(10, 10), p(0, 10)]))
  near(square.offset(-1)[0].area(), 64)
  near(square.offset(1)[0].area(), 140 + Math.PI)
  assert.deepEqual(square.offset(-6), [])
  const dumbbell = sx.Profile.new(sx.Wire2.polygon([[0, 0], [4, 0], [4, 1], [8, 1], [8, 0], [12, 0],
    [12, 4], [8, 4], [8, 3], [4, 3], [4, 4], [0, 4]].map(([x, y]) => p(x, y))))
  assert.equal(dumbbell.offset(-1.1).length, 2)
  const annulus = sx.Profile.new(sx.Wire2.new([sx.TrimmedCurve2.circle(p(0, 0), 5)]),
    [sx.Wire2.new([sx.TrimmedCurve2.circle(p(0, 0), 1)])])
  const expanded = annulus.offset(2)
  assert.equal(expanded[0].holes().length, 0)
  near(expanded[0].area(), Math.PI * 49)
})

test('classification distinguishes the outer boundary, holes and filled area', () => {
  const outer = sx.Wire2.polygon([p(0, 0), p(10, 0), p(10, 10), p(0, 10)])
  const hole = sx.Wire2.new([sx.TrimmedCurve2.circle(p(5, 5), 1)])
  const profile = sx.Profile.new(outer, [hole])
  assert.equal(profile.classifyPoint(p(0, 5)), 'boundary')
  assert.equal(profile.classifyPoint(p(5, 5)), 'outside')
  assert.equal(profile.classifyPoint(p(2, 2)), 'inside')
  assert.equal(profile.classifyPoint(p(6, 5)), 'boundary')
})

test('projection retains ellipses and reports curves collapsing to points', () => {
  const circle = sx.Circle3.new(sx.Point3.new(0, 0, 3), sx.Vec3.new(2, 0, 0), sx.Vec3.new(0, 1, Math.sqrt(3)))
  const spatial = sx.TrimmedCurve3.new(circle, sx.ParameterRange.new(0, Math.PI * 2))
  const projected = sx.Plane3.xy().projectCurve(spatial)
  assert.equal(projected.kind, 'curve')
  pointNear(projected.curve.evaluate(Math.PI / 2).point, p(0, 1))
  assert.throws(() => sx.Plane3.xy().expressCurve(spatial), sx.ShapexError)
  const vertical = sx.TrimmedCurve3.new(sx.Line3.through(sx.Point3.new(2, 3, 0), sx.Point3.new(2, 3, 10)), sx.ParameterRange.new(0, 1))
  const collapsed = vertical.projectedOnto(sx.Plane3.xy())
  assert.equal(collapsed.kind, 'point')
  pointNear(collapsed.point, p(2, 3))
  const scaled = sx.TrimmedCurve3.new(sx.Line3.new(sx.Point3.new(0, 0, 0), sx.Vec3.new(1e-9, 0, 0)), sx.ParameterRange.new(0, 1e10))
  const image = scaled.projectedOnto(sx.Plane3.xy())
  assert.equal(image.kind, 'curve')
  pointNear(image.curve.end(), p(10, 0))
})

test('planar transforms preserve geometry and structured diagnostics reach JavaScript', () => {
  const segment = sx.TrimmedCurve2.line(p(0, 0), p(2, 0))
  const transform = sx.RigidTransform2.new(Math.PI / 2, sx.Vec2.new(3, 4))
  pointNear(segment.transformed(transform).end(), p(3, 6))
  pointNear(transform.inverse().transformPoint(transform.transformPoint(p(7, 8))), p(7, 8))
  assert.throws(() => sx.TrimmedCurve2.circle(p(0, 0), 1).offset(2), error =>
    error instanceof sx.ShapexError && error.diagnostics[0].kind == 'InvalidInput')
})
