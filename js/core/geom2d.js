import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { AxialReference, CurveReference } from './references.js'
import {
  EPSILON,
  arrayRange,
  cross2d,
  rotationFromNormal,
  arrayFromOcVec,
  ocPnt2dFromVec,
  ocCirc2dFromVec,
  vecFromOc,
  ocCatch,
} from './utils.js'


export class SketchElement {
  constructor() {
    this.id = makeID()
    this.geom = null
  }

  typename(base) { return base + (this.projection ? ' Projection' : '') }

  clear() {
    if(this.geom) this.geom().get().Delete()
    this.geom = null
  }

  update(geom) {
    this.clear()
    geom = geom || this.geometry()
    this.geom = geom ? () => geom : null // Hide geometry from Vue
  }

  remove() {
    this.sketch.remove(this)
    this.clear()
  }

  isClosed() {
    return false
  }

  sample(u) {
    if(!this.geom) return
    const geom = this.geom().get()
    const start = geom.FirstParameter()
    const range = geom.LastParameter() - start
    const p = geom.Value(start + u * range)
    return vecFromOc(p)
  }

  unsample(p) {
    if(p instanceof THREE.Vector3) p = ocPnt2dFromVec(p)
    const projection = new window.oc.oc.Geom2dAPI_ProjectPointOnCurve_2(p, this.geom())
    return projection.LowerDistanceParameter()
  }

  distanceTo(p) {
    if(p instanceof THREE.Vector3) p = ocPnt2dFromVec(p)
    const projection = new window.oc.oc.Geom2dAPI_ProjectPointOnCurve_2(p, this.geom())
    return projection.NbPoints() ? projection.LowerDistance() : Infinity
  }

  endpoints() {
    return [this.sample(0.0), this.sample(1.0)]
  }

  otherBound(bound) {
    const points = this.endpoints()
    return bound.almost(points[0]) ? points[1] : points[0]
  }

  commonHandle(other) {
    if(!other.handles) return
    return this.handles().find(p => other.handles().some(op => p.almost(op) ) )
  }

  length() {
    if(!this.geom) return 0.0
    const adaptor = new window.oc.oc.Geom2dAdaptor_Curve_2(this.geom())
    return window.oc.oc.CPnts_AbscissaPoint.Length_2(adaptor)
  }

  tesselationSteps() { return 120 }

  tesselate() {
    if(!this.geom) return
    return tesselateCurveFixed(this.geom().get(), this.tesselationSteps())
  }

  intersect(others) {
    if(!this.geom) return []
    const geom = this.geom()
    return others.flatMap(other => {
      if(!other.geom || other == this) return []
      const isect = new window.oc.oc.Geom2dAPI_InterCurveCurve_2(geom, other.geom(), EPSILON / 10.0)// 1.0e-6)
      return arrayRange(1, isect.NbPoints()).map(i => isect.Point(i) )
    })
  }

  split(others) {
    const geom = this.geom()
    const start = geom.get().FirstParameter()
    const end = geom.get().LastParameter()
    const isCircle = (this.constructor == Circle)

    if(isCircle) {
      const period = end - start
      const intersections = this.intersect(others)
        .map(p => this.unsample(p) )
        // A circle is periodic, so its start and end parameters describe the
        // same point. Keep intersections at that seam and normalize them to
        // the start instead of discarding them.
        .map(u => start + (((u - start) % period) + period) % period )
        .map(u => u.almost(start) || (end - u).almost(0.0) ? start : u )
        .reduce((unique, u) => {
          if(!unique.some(other => other.almost(u) )) unique.push(u)
          return unique
        }, [])
        .sort((a, b) => a - b)
      if(!intersections.length) return [this]
      if(intersections.length < 2) return [this]
      const params = [...intersections, intersections[0]]
      const curves = ocCatch(() => params.slice(0, -1).map((param, i) =>
        new window.oc.oc.Geom2d_TrimmedCurve(geom, param, params[i + 1], true, true)
      ))
      return curves.map((curve, i) => Arc.fromGeometry(curve, this.id + '/' + i) )

    } else {
      const [p, u] = this
        .intersect(others)
        .map(p => [p, this.unsample(p)] )
        .find(([_p, u]) => !start.almost(u) && !end.almost(u) ) || []
      if(!p) return [this]
      const left = new window.oc.oc.Geom2d_TrimmedCurve(geom, start, u, true, true)
      const right = new window.oc.oc.Geom2d_TrimmedCurve(geom, u, end, true, true)
      return [...this.constructor.fromGeometry(left, this.id + '/1').split(others), ...this.constructor.fromGeometry(right, this.id + '/2').split(others)]
    }
  }

  constraints() {
    return this.sketch.constraints.filter(constraint => constraint.items.some(item => item.curve() == this ) )
  }

  clone() {
    const clone = this._clone()
    clone.id = this.id
    clone.sketch = this.sketch
    return clone
  }

  // toThree(material) {
  //   const positions = this.tesselate().flat()
  //   const geometry = new LineGeometry()
  //   geometry.setPositions(positions)
  //   // geometry.setColors(positions.map((pos, i) => i / positions.length ))
  //   geometry.setColors(Array(positions.length).fill(1))
  //   const line = new Line2(geometry, material)
  //   line.computeLineDistances()
  //   return line
  // }
}


export class Line extends SketchElement {
  typename() { return super.typename('Line') }

  constructor(p1, p2, geom) {
    super()
    this.points = [p1, p2]
    this.update(geom)
  }

  static fromGeometry(geom, id) {
    const line = new Line(vecFromOc(geom.StartPoint()), vecFromOc(geom.EndPoint()), new window.oc.oc.Handle_Geom2d_Curve_2(geom))
    line.id = id
    return line
  }

  snapPoints() {
    return [...this.points, this.midpoint()]
  }

  endpoints() {
    return this.points
  }

  midpoint() {
    return this.points[0].clone().add(this.points[1]).divideScalar(2.0)
  }

  center() { return this.midpoint() }

  handles() {
    return this.points
  }

  setHandles(handles) {
    this.points = handles
    this.update()
  }

  geometry() {
    const [p1, p2] = this.points
    if(p1.almost(p2)) return
    return new window.oc.oc.Handle_Geom2d_Curve_2(new window.oc.oc.GCE2d_MakeSegment_1(ocPnt2dFromVec(p1), ocPnt2dFromVec(p2)).Value().get())
  }

  flip() {
    this.points = [this.points[1], this.points[0]]
    this.update()
  }

  tesselationSteps() { return 2 }

  // tesselate() {
  //   return this.points.map(p => p.toArray() )
  // }

  // length() {
  //   return this.points[0].distanceTo(this.points[1])
  // }

  direction() {
    return this.points[1].clone().sub(this.points[0])
  }

  axialReference() {
    return new AxialReference(new CurveReference(this))
  }

  getAxis() {
    return rotationFromNormal(this.direction().normalize()).setPosition(this.points[0]).premultiply(this.sketch.workplane)
  }

  _clone() {
    return new Line(...this.points)
  }

  dump() {
    return {
      id: this.id,
      points: this.points,
    }
  }

  static undump(dump) {
    const line = new Line(...dump.points)
    line.id = dump.id
    return line
  }
}

Serialize.register(Line, 'Line')


export class Circle extends SketchElement {
  typename() { return super.typename('Circle') }

  constructor(center, radius, geom) {
    super()
    this._center = center
    this.radius = radius
    this.update(geom)
  }

  static fromGeometry(geom, id) {
    const basis = geom.BasisCurve().get()
    const circle = new Circle(vecFromOc(basis.Location()), basis.Radius(), new window.oc.oc.Handle_Geom2d_Curve_2(geom))
    circle.id = id
    return circle
  }

  snapPoints() {
    return [this._center]
  }

  center() { return this._center.clone() }

  handles() {
    return [this._center]
  }

  setHandles(handles) {
    this._center = handles[0]
    if(handles[1]) this.radius = handles[0].distanceTo(handles[1])
    this.update()
  }

  geometry() {
    const center = ocPnt2dFromVec(this._center)
    const v = new window.oc.oc.gp_Dir2d_4(1.0, 0.0)
    const axis = new window.oc.oc.gp_Ax2d_2(center, v)
    return new window.oc.oc.Handle_Geom2d_Curve_2(new window.oc.oc.Geom2d_Circle_2(axis, this.radius, false))
  }

  isClosed() {
    return true
  }

  // length() {
  //   return this.radius * 2.0 * Math.PI
  // }

  area() {
    return Math.PI * Math.pow(this.radius, 2.0)
  }

  axialReference() {
    return new AxialReference(this)
  }

  getAxis() {
    return this.sketch.workplane.clone()
  }

  _clone() {
    return new Circle(this._center, this.radius)
  }

  dump() {
    return {
      id: this.id,
      _center: this._center,
      radius: this.radius,
    }
  }

  static undump(dump) {
    const circle = new Circle(dump._center, dump.radius)
    circle.id = dump.id
    return circle
  }
}

Serialize.register(Circle, 'Circle')


export class Arc extends SketchElement {
  typename() { return super.typename('Arc') }

  constructor(center, radius, bounds, forward=true, geom) {
    super()
    this._center = center
    this.radius = radius
    this.bounds = bounds
    this.forward = forward
    this.update(geom)
  }

  static fromPoints(points) {
    const arc = new window.oc.oc.GCE2d_MakeArcOfCircle_4(
      ocPnt2dFromVec(points[0]),
      ocPnt2dFromVec(points[1]),
      ocPnt2dFromVec(points[2]),
    )
    if(!arc.IsDone()) return
    const trimmed = arc.Value().get()
    // const basis = new window.oc.oc.Geom2dAdaptor_Curve_2(trimmed.BasisCurve())
    // const circle = basis.Circle()
    const circle = trimmed.BasisCurve().get()
    const center = vecFromOc(circle.Location())
    const radius = circle.Radius()
    const bounds = [trimmed.FirstParameter(), trimmed.LastParameter()]
    return new Arc(center, radius, bounds, true, new window.oc.oc.Handle_Geom2d_Curve_2(trimmed))
  }

  static fromGeometry(geom, id) {
    const basis = geom.BasisCurve().get()
    const arc = new Arc(
      vecFromOc(basis.Location()),
      basis.Radius(),
      [geom.FirstParameter(), geom.LastParameter()],
      true,
      new window.oc.oc.Handle_Geom2d_Curve_2(geom)
    )
    arc.id = id
    return arc
  }

  sample(u) {
    return super.sample(this.forward ? u : 1.0 - u)
  }

  center() { return this._center.clone() }

  snapPoints() {
    return [this._center, ...this.endpoints()]
  }

  handles() {
    return [this._center, ...this.endpoints()]
  }

  setHandles(handles) {
    // The three-point constructor used while drawing can create an arc in
    // either direction. Preserve that direction when the solver writes the
    // center and endpoints back; otherwise forcing `Sense` to true chooses
    // the opposite side for counter-clockwise arcs.
    const startRadius = this.sample(0.0).sub(this._center)
    const nextRadius = this.sample(1.0e-4).sub(this._center)
    const clockwise = cross2d(startRadius, nextRadius) < 0.0

    this._center = handles[0]
    const circ = ocCirc2dFromVec(this._center, this.radius)
    // Keep the stored curve on the canonical clockwise circle used by
    // geometry(), cloning and serialization. Counter-clockwise arcs use the
    // same curve with reversed endpoints and logical sampling direction.
    const endpoints = clockwise ? handles.slice(1) : handles.slice(1).reverse()
    const arc = new window.oc.oc.GCE2d_MakeArcOfCircle_3(circ, ocPnt2dFromVec(endpoints[0]), ocPnt2dFromVec(endpoints[1]), true)
    if(!arc.IsDone()) return
    const trimmed = arc.Value().get()
    const circle = trimmed.BasisCurve().get()
    this.radius = circle.Radius()
    this.bounds = [trimmed.FirstParameter(), trimmed.LastParameter()]
    this.forward = clockwise
    this.update(new window.oc.oc.Handle_Geom2d_Curve_2(trimmed))
  }

  setPoints(points) {
    const arc = Arc.fromPoints(points)
    this._center = arc._center
    this.radius = arc.radius
    this.bounds = arc.bounds
    this.forward = arc.forward
    this.geom = arc.geom
  }

  geometry() {
    const circ = ocCirc2dFromVec(this._center, this.radius)
    return new window.oc.oc.Handle_Geom2d_Curve_2(new window.oc.oc.GCE2d_MakeArcOfCircle_1(circ, this.bounds[0], this.bounds[1], true).Value().get())
  }

  isClosed() {
    return false
  }

  update(geom) {
    super.update(geom)
    geom = this.geom().get()
    this.bounds = [geom.FirstParameter(), geom.LastParameter()]
  }

  flip() {
    this.forward = !this.forward
  }

  tesselate() {
    const tess = super.tesselate()
    if(!this.forward) tess.reverse()
    return tess
  }

  _clone() {
    return new Arc(this._center, this.radius, this.bounds, this.forward)
  }

  dump() {
    return {
      id: this.id,
      _center: this._center,
      radius: this.radius,
      bounds: this.bounds,
      forward: this.forward,
    }
  }

  static undump(dump) {
    const arc = new Arc(dump._center, dump.radius, dump.bounds, dump.forward)
    arc.id = dump.id
    return arc
  }
}
Serialize.register(Arc, 'Arc')


export class Spline extends SketchElement {
  typename() { return super.typename('Spline') }

  constructor(points, geom) {
    super()
    this.points = points
    this.update(geom)
  }

  static clampedKnots(n, degree) {
    if(degree >= n) { return [] }
    const d = degree + 1
    const uniform = arrayRange(1, n - d)
    const last = n - d + 1
    return [
      [0.0, ...uniform, last],
      [d, ...Array(uniform.length).fill(1), d],
    ]
  }

  endpoints() {
    return [this.points[0], this.points.slice(-1)[0]]
  }

  snapPoints() {
    return this.endpoints()
  }

  handles() {
    return [...this.points]
  }

  setHandles(handles) {
    this.points = handles
    this.update()
  }

  center() { return this.sample(0.5) }

  geometry() {
    const n = this.points.length
    if(n < 2) throw "Splines need at least two control vertices"

    const degree = Math.min(5, n - 1)
    const [knots, multiplicities] = Spline.clampedKnots(n, degree)

    const points = new window.oc.oc.TColgp_Array1OfPnt2d_2(1, this.points.length)
    this.points.forEach((p, i) => points.SetValue(i + 1, ocPnt2dFromVec(p)) )

    const knts = new window.oc.oc.TColStd_Array1OfReal_2(1, knots.length)
    knots.forEach((knot, i) => knts.SetValue(i + 1, knot) )

    const mults = new window.oc.oc.TColStd_Array1OfInteger_2(1, multiplicities.length)
    multiplicities.forEach((mult, i) => mults.SetValue(i + 1, mult) )

    const spline = new window.oc.oc.Geom2d_BSplineCurve_1(points, knts, mults, degree, false)

    return new window.oc.oc.Handle_Geom2d_Curve_2(spline)
  }

  flip() {
    this.points.reverse()
    this.update()
  }

  _clone() {
    return new Spline([...this.points])
  }

  dump() {
    return {
      id: this.id,
      points: this.points,
    }
  }

  static undump(dump) {
    const line = new Spline(dump.points)
    line.id = dump.id
    return line
  }
}

Serialize.register(Spline, 'Spline')


function tesselateCurveFixed(geom, steps) {
  const start = geom.FirstParameter()
  const range = geom.LastParameter() - start

  const vertices = arrayRange(0, steps - 1).map(i => {
    const u = start + i / (steps - 1) * range
    const p = geom.Value(u)
    return arrayFromOcVec(p)
  })
  return vertices
}
