import * as THREE from 'three'
import earcut from 'earcut'

import { arrayRange } from '../utils.js'
import Component from './component.js'
import { CreateComponentFeature, Feature } from './features.js'

// Sketches

export class SketchElement {
  constructor() {
    this.id = crypto.randomUUID()
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

  endpoints() {
    return [this.sample(0.0), this.sample(1.0)]
  }

  otherBound(bound) {
    const points = this.endpoints()
    return bound.almost(points[0]) ? points[1] : points[0]
  }

  commonHandle(other) {
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
      const isect = new window.oc.oc.Geom2dAPI_InterCurveCurve_2(geom, other.geom(), 1.0e-6)
      return arrayRange(1, isect.NbPoints()).map(i => isect.Point(i) )
    })
  }

  split(others) {
    const geom = this.geom()
    const start = geom.get().FirstParameter()
    const end = geom.get().LastParameter()
    const [p, u] = this
      .intersect(others)
      .map(p => [p, this.unsample(p)] )
      .find(([_p, u]) => !start.almost(u) && !end.almost(u) ) || []
    if(!p) return [this]
    const left = new window.oc.oc.Geom2d_TrimmedCurve(geom, start, u, true, true)
    const right = new window.oc.oc.Geom2d_TrimmedCurve(geom, u, end, true, true)
    return [...this.constructor.fromGeometry(left, this.id + '/1').split(others), ...this.constructor.fromGeometry(right, this.id + '/2').split(others)]
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
    return new Line(this.points[0], this.points[1])
  }
}


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
    const center = new window.oc.oc.gp_Pnt2d_3(this._center.x, this._center.y)
    const v = new window.oc.oc.gp_Dir2d_4(0.0, 1.0)
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

  _clone() {
    return new Circle(this._center, this.radius)
  }
}


export class Arc extends SketchElement {
  typename() { return super.typename('Arc') }

  constructor(center, radius, bounds, geom) {
    super()
    this._center = center
    this.radius = radius
    this.bounds = bounds
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
    const basis = new window.oc.oc.Geom2dAdaptor_Curve_2(trimmed.BasisCurve())
    const circle = basis.Circle()
    const center = vecFromOc(circle.Location())
    const radius = circle.Radius()
    const bounds = [trimmed.FirstParameter(), trimmed.LastParameter()]
    return new Arc(center, radius, bounds, new window.oc.oc.Handle_Geom2d_Curve_2(trimmed))
  }

  static fromGeometry(geom, id) {
    const basis = geom.BasisCurve().get()
    const arc = new Arc(vecFromOc(basis.Location()), basis.Radius(), basis.FirstParameter(), basis.LastParameter(), new window.oc.oc.Handle_Geom2d_Curve_2(geom))
    arc.id = id
    return arc
  }

  center() { return this._center.clone() }

  snapPoints() {
    return [this._center, ...this.endpoints()]
  }

  handles() {
    return [this._center, ...this.endpoints()]
  }

  setHandles(handles) {
    this._center = handles[0]
    const circ = ocCirc2dFromVec(this._center, this.radius)
    const arc = new window.oc.oc.GCE2d_MakeArcOfCircle_3(circ, ocPnt2dFromVec(handles[1]), ocPnt2dFromVec(handles[2]), true)
    if(!arc.IsDone()) return
    const trimmed = arc.Value().get()
    const basis = new window.oc.oc.Geom2dAdaptor_Curve_2(trimmed.BasisCurve())
    const circle = basis.Circle()
    this.radius = circle.Radius()
    this.bounds = [trimmed.FirstParameter(), trimmed.LastParameter()]
    this.update(new window.oc.oc.Handle_Geom2d_Curve_2(trimmed))
  }

  setPoints(points) {
    const arc = Arc.fromPoints(points)
    this._center = arc._center
    this.radius = arc.radius
    this.bounds = arc.bounds
    this.geom = arc.geom
  }

  geometry() {
    const circ = ocCirc2dFromVec(this._center, this.radius)
    return new window.oc.oc.Handle_Geom2d_Curve_2(new window.oc.oc.GCE2d_MakeArcOfCircle_1(circ, this.bounds[0], this.bounds[1], true).Value().get())
  }

  isClosed() {
    return false
  }

  flip() {
    this.bounds = [this.bounds[1], this.bounds[0]]
    this.update()
  }

  _clone() {
    return new Arc(this._center, this.radius, this.bounds)
  }
}


export class Sketch {
  constructor() {
    this.id = crypto.randomUUID()
    this.elements = []
    this.constraints = []
    this.projections = []
    this.workplane = new THREE.Matrix4()
    // this.componentId = componentId
  }

  typename() { return 'Sketch' }

  add(elem) {
    this.elements.push(elem)
    elem.sketch = this
  }

  addConstraint(constraint) {
    constraint.sketch = this
    this.constraints.push(constraint)
  }

  addProjection(projection) {
    projection.sketch = this
    this.projections.push(projection)
  }

  remove(elem) {
    if(elem instanceof SketchElement) {
      this.elements = this.elements.filter(e => e != elem )
      this.constraints = this.constraints.filter(c => !c.items.some(item => item.curve() == elem ) )
      if(elem.projection) this.projections = this.projections.filter(p => p != elem.projection )

    } else if(elem instanceof Constraint) {
      this.constraints = this.constraints.filter(c => c != elem )
    }
  }

  profiles(comp, includeOuter) {
    const elements = this.removeEmpties(this.elements)
    // console.log('elements', elements)
    const cutElements = elements.flatMap(elem => elem.split(elements) )
    // console.log('cutElements', cutElements)
    const wires = this.getWires(cutElements, includeOuter)
    // console.log('wires', wires)
    const profiles = this.buildProfiles(comp, wires)

    return profiles
  }

  removeEmpties(elements) {
    return elements.filter(curve => !curve.isReference && !curve.length().almost(0.0) )
  }

  getWires(cutElements, includeOuter) {
    const circles = cutElements.filter(elem => elem.isClosed() )
    let others = cutElements.filter(elem => !elem.isClosed() )
    // console.log('others', others)
    others = this.removeDanglingSegments(others)
    // console.log('others2', others)
    let islands = this.buildIslands(others)
    // console.log('islands', islands)
    let wires = islands.flatMap(island => this.buildWiresFromIsland(island, includeOuter) )
    // console.log('wires2', wires)
    const circleWires = circles.map(circle => new Wire([circle]) )
    wires = wires.concat(circleWires)
    return wires
  }

  removeDanglingSegments(island) {
    let others = [...island]
    let startLen = island.length
    island = island.filter(elem => {
      if(elem.length().almost(0.0)) return false
      // Keep closed circles, arcs and splines
      if(elem.isClosed()) return true
      let [startPoint, endPoint] = elem.endpoints()
      return [startPoint, endPoint].every(endpoint => {
        return others.some(otherElem => {
          let [otherStart, otherEnd] = otherElem.endpoints()
          return (endpoint.almost(otherStart) || endpoint.almost(otherEnd)) && otherElem.id != elem.id
        })
      })
    })
    if(island.length < startLen) { island = this.removeDanglingSegments(island) }
    return island
  }

  buildWiresFromIsland(island, includeOuter) {
    let wires = []
    let usedForward = new Set()
    let usedBackward = new Set()
    // console.log('buildWiresFromIsland island', island)
    island.forEach(startElem => {
      // let points = tuple2_to_vec(startElem.endpoints())
      startElem.endpoints().forEach(point => {
        let loops = this.buildLoop(
          point,
          startElem,
          [],
          island,
          usedForward,
          usedBackward,
        )
        let newWires = loops.map(region => new Wire(region) )
        wires = wires.concat(newWires)
      })
    })
    // console.log('buildWiresFromIsland wires1', wires)
    if(!includeOuter) { wires = this.removeOuterLoop(wires) }
    return wires
  }

  removeOuterLoop(loops) {
    if(loops.length <= 1) return loops
    return loops.filter(wire => !wire.isClockwise() )
  }

  buildProfiles(comp, wires) {
    return wires.map(wire => {
      // Find all other wires enclosed by this one
      let cutouts = []
      wires.forEach(other => {
        // if ptr::eq(&*wire, &*other) { continue }
        if(wire !== other && wire.encloses(other)) {
          cutouts.push(other)
        }
      })
      let profile = [wire]
      // Only leave the outermost inner wires
      profile = profile.concat(cutouts.filter(cutout =>
        !cutouts.some(other =>
          // !ptr::eq(&cutout[0], &other[0]) && other.encloses(cutout)
          cutout[0] !== other[0] && other.encloses(cutout)
        )
      ))
      return new Profile(this, profile)
    })
  }

  buildLoop(
    startPoint,
    startElem,
    path,
    allElements,
    usedForward,
    usedBackward,
  ) {
    let regions = []
    // Traverse edges only once in every direction
    let startElemId = startElem.id
    if(startPoint.almost(startElem.endpoints()[0])) {
      if(usedForward.has(startElemId)) return regions
      usedForward.add(startElemId)
    } else {
      if(usedBackward.has(startElemId)) return regions
      usedBackward.add(startElemId)
    }
    // Add startElem to path
    path.push(startElem)
    // Find connected segments
    let endPoint = startElem.otherBound(startPoint)
    let connectedElems = allElements.filter(otherElem => {
      let [otherStart, otherEnd] = otherElem.endpoints()
      return (endPoint.almost(otherStart) || endPoint.almost(otherEnd)) &&
        otherElem.id != startElemId
    })
    if(connectedElems.length) {
      // Sort connected segments in clockwise order
      connectedElems.sort((a, b) => { //XXX min_by_key
        let finalPointA = a.otherBound(endPoint)
        let finalPointB = b.otherBound(endPoint)
        return clockwise(startPoint, endPoint, finalPointB) > clockwise(startPoint, endPoint, finalPointA)
      })
      // Follow the leftmost segment to complete loop in anti-clockwise order
      let nextElem = connectedElems[0]
      if(path[0].id == nextElem.id) {
        // We are closing a loop
        regions.push(path)
      } else {
        // Follow loop
        let newRegions = this.buildLoop(
          endPoint,
          nextElem,
          path,
          allElements,
          usedForward,
          usedBackward,
        )
        regions = regions.concat(newRegions)
      }
    }
    return regions
  }

  buildIslands(elements) {
    let unusedElements = [...elements]
    let islands = []
    while(unusedElements.length) {
      let startElem = unusedElements.pop()
      let island = []
      this.buildIsland(startElem, island, unusedElements)
      // console.log('buildIslands island', island)
      island.forEach(islandElem => {
        unusedElements = unusedElements.filter(elem => elem.id != islandElem.id )
      })
      if(island.length > 0) islands.push(island)
    }
    return islands
  }

  buildIsland(startElem, path, allElements) {
    if(path.some(e => e == startElem )) return
    let [startPoint, endPoint] = startElem.endpoints()
    path.push(startElem)
    allElements.forEach(elem => {
      let [otherStart, otherEnd] = elem.endpoints()
      // We are connected to other element
      if(endPoint.almost(otherStart) ||
         endPoint.almost(otherEnd) ||
         startPoint.almost(otherStart) ||
         startPoint.almost(otherEnd))
      {
        this.buildIsland(elem, path, allElements)
      }
    })
  }

  solve(tree) {
    let id = 1
    const idMap = {}

    // Convert entire sketch to GCS format
    const projections = this.projections.map(p => p.update(tree) ).filter(Boolean)
    const primitives = this.elements.concat(projections).flatMap(elem => {
      let primitives

      if(elem instanceof Line) {
        const p1 = { id: `${id++}`, type: 'point', x: elem.points[0].x, y: elem.points[0].y, fixed: elem.projected }
        const p2 = { id: `${id++}`, type: 'point', x: elem.points[1].x, y: elem.points[1].y, fixed: elem.projected }
        const line = { id: `${id++}`, type: 'line', p1_id: p1.id, p2_id: p2.id }
        primitives = [p1, p2, line]

      } else if(elem instanceof Circle) {
        const center = { id: `${id++}`, type: 'point', x: elem._center.x, y: elem._center.y, fixed: elem.projected }
        const circle = { id: `${id++}`, type: 'circle', c_id: center.id, radius: elem.radius }
        primitives = [center, circle]

      } else if(elem instanceof Arc) {
        const center = { id: `${id++}`, type: 'point', x: elem._center.x, y: elem._center.y, fixed: elem.projected }
        const endpoints = elem.endpoints()
        const start =  { id: `${id++}`, type: 'point', x: endpoints[0].x, y: endpoints[0].y, fixed: elem.projected }
        const end =    { id: `${id++}`, type: 'point', x: endpoints[1].x, y: endpoints[1].y, fixed: elem.projected }
        const arc = {
          id: `${id++}`,
          type: 'arc',
          c_id: center.id,
          radius: elem.radius,
          start_angle: elem.geom().get().FirstParameter(),
          end_angle: elem.geom().get().LastParameter(),
          start_id: start.id,
          end_id: end.id,
        }
        // const rules = { id: `${id++}`, type: 'arc_rules', a_id: arc.id  }
        primitives = [start, end, center, arc]
      }

      idMap[elem.id] = primitives
      return primitives
    }).filter(Boolean)

    const constraints = this.constraints.flatMap(c => {

      c.update(tree)
      // Strictly only necessary for DimensionControls to reference the updated projections.
      // The solver itself works without it.

      // Single curve constraints
      if(c instanceof HorizontalConstraint) {
        const pointPrims = idMap[c.items[0].curve().id].slice(0, 2)
        return { id: `${id++}`, type: 'horizontal_pp', p1_id: pointPrims[0].id, p2_id: pointPrims[1].id, temporary: c.temporary }

      } else if(c instanceof VerticalConstraint) {
        const pointPrims = idMap[c.items[0].curve().id].slice(0, 2)
        return { id: `${id++}`, type: 'vertical_pp', p1_id: pointPrims[0].id, p2_id: pointPrims[1].id, temporary: c.temporary }

      } else if(c instanceof FixConstraint) {
        const pointPrims = idMap[c.items[0].curve().id].slice(0, 2)
        return [
          { id: `${id++}`, type: 'coordinate_x', p_id: pointPrims[0].id, x: pointPrims[0].x },
          { id: `${id++}`, type: 'coordinate_y', p_id: pointPrims[0].id, y: pointPrims[0].y },
          { id: `${id++}`, type: 'coordinate_x', p_id: pointPrims[1].id, x: pointPrims[1].x },
          { id: `${id++}`, type: 'coordinate_y', p_id: pointPrims[1].id, y: pointPrims[1].y },
        ]

      // Pair constraints
      } else if(c instanceof PerpendicularConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        return { id: `${id++}`, type: 'perpendicular_ll', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

      } else if(c instanceof ParallelConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        return { id: `${id++}`, type: 'parallel', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

      } else if(c instanceof EqualConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        // Equal radius circle/circle
        if(c.items[0].curve() instanceof Circle) {
          return { id: `${id++}`, type: 'equal_radius_cc', c1_id: constraintPrims[0].id, c2_id: constraintPrims[1].id, temporary: c.temporary }

        // Equal length line/line
        } else {
          return { id: `${id++}`, type: 'equal_length', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

        }

      } else if(c instanceof TangentConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        return { id: `${id++}`, type: 'tangent_lc', l_id: constraintPrims[0].id, c_id: constraintPrims[1].id, temporary: c.temporary }

      // Point constraints
      } else if(c instanceof CoincidentConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id][item.index] )
        return { id: `${id++}`, type: 'p2p_coincident', p1_id: constraintPrims[0].id, p2_id: constraintPrims[1].id, temporary: c.temporary }

      // Dimension
      } else if(c instanceof Dimension) {
        // Circle diameter
        if(c.items[0].curve() instanceof Circle) {
          const circlePrim = idMap[c.items[0].curve().id].slice(-1)[0]
          return { id: `${id++}`, type: 'circle_diameter', c_id: circlePrim.id, diameter: c.distance, temporary: c.temporary }

        } else if(c.items[0].curve() instanceof Arc) {
          const arcPrim = idMap[c.items[0].curve().id].slice(-1)[0]
          return { id: `${id++}`, type: 'arc_radius', a_id: arcPrim.id, radius: c.distance, temporary: c.temporary }

        // Line to line distance
        } else {
          const pointPrim = idMap[c.items[0].curve().id][0]
          const linePrim = idMap[c.items[1].curve().id].slice(-1)[0]
          return { id: `${id++}`, type: 'p2l_distance', p_id: pointPrim.id, l_id: linePrim.id, distance: c.distance, temporary: c.temporary }
        }
      }
    })

    // Solve
    const { results, conflicting, _dof } = window.oc.solveSystem([...primitives, ...constraints])

    if(conflicting) {
      window.bus.emit('toast', 'Sketch was over-constrained')
      this.constraints.pop()
      this.solve(tree)
      return
    }

    // Write back results
    const updatePrim = (prim) => results.find(res => res.id == prim.id )

    const vecFromPrim = (prim) => {
      const updated = updatePrim(prim)
      return new THREE.Vector3(updated.x, updated.y, 0.0)
    }

    this.elements.forEach(elem => {
      if(elem instanceof Line) {
        const [p1, p2] = idMap[elem.id]
        elem.setHandles([vecFromPrim(p1), vecFromPrim(p2)])

      } else if(elem instanceof Circle) {
        const [center, circle] = idMap[elem.id]
        elem.radius = updatePrim(circle).radius
        elem.setHandles([vecFromPrim(center)])

      } else if(elem instanceof Arc) {
        const [start, end, center, _arc, _rules] = idMap[elem.id]
        elem.setHandles([vecFromPrim(center), vecFromPrim(start), vecFromPrim(end)])
      }
    })
  }
}


export class Projection {
  constructor(edge) {
    this.edgeRef = new EdgeReference(edge)
  }

  update(tree) {
    this.edgeRef.update(tree)
    const edge = this.edgeRef.getItem()
    // BRep_Tool.Curve()
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(edge.geom())
    const ax = ocAx3FromMatrix(this.sketch.workplane)
    const plane = ocPlnFromMatrix(this.sketch.workplane)

    const project = new window.oc.oc.ProjLib_ProjectOnPlane_2(ax)
    project.Load(curve.ShallowCopy(), EPSILON, true) // ShallowCopy upcasts BRepAdaptor_Curve -> Adaptor3d_Curve

    const projected = project.GetResult().get()
    const [u1, u2] = [projected.FirstParameter(), projected.LastParameter()]

    let curve2d = window.oc.oc.GeomAPI.To2d(projected.Curve(), plane)
    const handle = new window.oc.oc.Handle_Geom2d_Curve_2(curve2d.get())

    const trimmed = new window.oc.oc.Geom2d_TrimmedCurve(handle, u1, u2, true, true)

    if(project.GetType() == window.oc.oc.GeomAbs_CurveType.GeomAbs_Line) {
      const line = Line.fromGeometry(trimmed, edge.id)
      line.sketch = this.sketch
      line.projection = this
      this.output = line
      return this.output
    }
  }

  geometry() {
    return this.output
  }
}


export class ElemRef {
  constructor(curve, index) {
    this.curveRef = new CurveReference(curve)
    this.index = index
  }

  update(tree) {
    this.curveRef.update(tree)
  }

  curve() {
    return this.curveRef.getItem()
  }

  isPoint() { return this.index !== undefined }
}

export class Constraint {
  constructor(...items) {
    this.items = items.map(item => item instanceof ElemRef ? item : new ElemRef(item) )
  }

  update(tree) {
    this.items.forEach(item => item.update(tree) )
  }
}

export class HorizontalConstraint extends Constraint {
  static icon = 'ruler-horizontal'
  typename() { return 'Horizontal Constraint' }
}

export class VerticalConstraint extends Constraint {
  static icon = 'ruler-vertical'
  typename() { return 'Vertical Constraint' }
}

export class FixConstraint extends Constraint {
  static icon = 'lock'
  typename() { return 'Fix Constraint' }
}

export class CoincidentConstraint extends Constraint {
  static icon = 'bullseye'
  typename() { return 'Coincident Constraint' }
}

export class PerpendicularConstraint extends Constraint {
  static icon = 'angle-up'
  typename() { return 'Perpendicular Constraint' }
}

export class ParallelConstraint extends Constraint {
  static icon = 'exchange-alt'
  typename() { return 'Parallel Constraint' }
}

export class EqualConstraint extends Constraint {
  static icon = 'equals'
  typename() { return 'Equal Constraint' }
}

export class TangentConstraint extends Constraint {
  static icon = 'bezier-curve'
  typename() { return 'Tangent Constraint' }
}

export class Dimension extends Constraint {
  static icon = 'ruler'
  typename() { return 'Dimension' }

  constructor(items, pos) {
    super(...items)
    this.position = pos
    if(items[0] instanceof Circle) {
      this.distance = items[0].radius * 2.0

    } else if(items[0] instanceof Arc) {
      this.distance = items[0].radius

    } else {
      const [a, b] = items
      const aPoint = a.handles()[0].clone().sub(b.handles()[0])
      const dir = b.direction()
      this.distance = aPoint.clone().projectOnVector(dir).distanceTo(aPoint)
    }
  }
}


export class Wire {
  constructor(region) {
    region = region.map(seg => seg.clone() )

    if(region.length >= 2) {
      // Find starting point from element order
      let bounds = region[0].endpoints()
      let next_bounds = region[1].endpoints()
      let point = (bounds[0].almost(next_bounds[0]) || bounds[0].almost(next_bounds[1])) ? bounds[1] : bounds[0]

      // Flip curves to flow consistently along element order
      region.forEach(tcurve => {
        if(tcurve.endpoints()[1].almost(point)) {
          point = tcurve.endpoints()[0]
          tcurve.flip()
        } else {
          point = tcurve.endpoints()[1]
        }
      })
    }

    if(region.length == 0) throw "Wires may not be empty"
    let firstPoint = region[0].endpoints()[0]
    let lastPoint = region.slice(-1)[0].endpoints()[1]
    if(!firstPoint.almost(lastPoint)) throw "Wires must be closed"

    this.segments = region
  }

  isClockwise() {
    const cage = this.cage()
    return isClockwise(cage)
  }

  reverse() {
    this.segments.reverse()
    this.segments.forEach(tcurve => tcurve.flip() )
  }

  cage() {
    let polyline = this.segments.map(curve => curve.sample(0.0) )
    // polyline.push(this.segments[0].endpoints()[0])
    return polyline
  }

  containsPoint(p) {
    const ray = new Line(p, p.clone().add(new THREE.Vector3(999999.0, 0.0, 0.0)))
    // const perElem = this.segments.map(elem => ray.intersect([elem]) )
    // const num_hits = perElem.iter().enumerate().flatMap((i, intersections) => {
    //   intersections.iter().map(isect => match isect {
    //     Cross(_) => 1,
    //     Pierce(_) => {
    //       let j = (i + 1) % this.segments.length
    //       let next_intersections = perElem[j];
    //       if i != j && next_intersections.iter().any(next_isect => matches!(next_isect, Pierce(_)) ) {
    //         0
    //       } else {
    //         1
    //       }
    //     },
    //     _ => 0,
    //   })
    // }).sum()
    const numHits = this.segments.flatMap(elem => ray.intersect([elem]) ).length
    return numHits % 2 != 0
  }

  encloses(other) {
    return other.segments.every(elem => this.containsPoint(elem.endpoints()[0]) )
  }

  tesselate() {
    let polyline = this.segments.flatMap(curve => {
      let poly = curve.tesselate()
      poly.pop()
      return poly
    })
    return polyline
  }

  geometry() {
    let wire = new window.oc.oc.BRepBuilderAPI_MakeWire_1()
    this.segments.forEach(seg => {
      const geom = seg.geom()
      const plane = new window.oc.oc.gp_Pln_1()
      const curve = window.oc.oc.GeomAPI.To3d(geom, plane)
      const edge = new window.oc.oc.BRepBuilderAPI_MakeEdge_24(curve).Edge()
      wire.Add_1(edge)
    })
    wire = wire.Wire()
    window.oc.oc.BOPTools_AlgoTools.OrientEdgesOnWire(wire)
    return wire
  }

  transformed(workplane) {
    const shape = transformGeometry(this.geometry(), workplane)
    return window.oc.oc.TopoDS.Wire_1(shape)
  }
}


export class Profile {
  constructor(sketch, rings) {
    // this.component = comp
    this.sketch = sketch
    this.rings = rings
    this.id = crypto.randomUUID()
  }

  reference() {
    return new ProfileReference(this)
  }

  tesselate() {
    let polyRings = this.rings.map(wire => wire.tesselate() )
    const flatVertices = polyRings[0].map(p => [p[0], p[1]] ).flat()
    var vertices = earcut(flatVertices, [], 2).map(v => polyRings[0][v] )
    return {
      positions: vertices.flat(),
      normals: vertices.flatMap(v => [0.0, 0.0, 1.0] ),
    }
  }

  containsPoint(p) {
    return this.rings[0].containsPoint(p) && !this.rings.slice(1).some(wire => wire.containsPoint(p) )
  }

  center() {
    const out = new THREE.Vector3()
    this.rings[0].segments.forEach(seg => out.add(seg.center()) )
    out.divideScalar(this.rings[0].segments.length)
    return out.applyMatrix4(this.sketch.workplane)
  }

  normal() {
    const rot = new THREE.Quaternion().setFromRotationMatrix(this.sketch.workplane)
    return new THREE.Vector3(0,0,1).applyQuaternion(rot)
  }

  extrude(componentId, height) {
    if(!height) throw { type: 'error', msg: 'Extrusion has no volume' }
    const face = this.makeFace()
    const rot = new THREE.Quaternion().setFromRotationMatrix(this.sketch.workplane)
    const dir = ocVecFromVec(new THREE.Vector3(0,0,height).applyQuaternion(rot))
    let prism = new window.oc.oc.BRepPrimAPI_MakePrism_1(face, dir, true, true) //XXX use BRepFeat_MakePrism to allow extrusion up to limit face
    return this.makeCompound(componentId, prism.Shape())
  }

  revolve(componentId, axis, angle) {
    if(!angle) throw { type: 'error', msg: 'Revolution has no volume' }
    const face = this.makeFace()
    const ax = ocAx1FromMatrix(axis)
    let revolution = new window.oc.oc.BRepPrimAPI_MakeRevol_1(face, ax, angle, true)
    return this.makeCompound(componentId, revolution.Shape())
  }

  makeFace() {
    const wire = this.rings[0].transformed(this.sketch.workplane)
    return new window.oc.oc.BRepBuilderAPI_MakeFace_15(wire, true).Face()
  }

  makeCompound(componentId, shape) {
    const solid = window.oc.oc.TopoDS.Solid_1(shape)
    const compound = new Compound(componentId, solid)
    compound.id = this.id
    return compound
  }

  update() {
    const cutElements = this.sketch.elements.flatMap(elem => elem.split(this.sketch.elements) )
    const newWires = this.sketch.getWires(cutElements, false)
    let wasRepairNeeded = false
    let error
    this.rings = this.rings.map(wire => {
      const wireIds = new Set(wire.segments.map(seg => seg.id ))
      const replacementWire = newWires.map(newWire => {
        const newWireIds = new Set(newWire.segments.map(tcurve => tcurve.id ))
        const count = wireIds.intersection(newWireIds).size
        if(count > 0) return [count, newWire]
      }).filter(Boolean).minMaxBy(Math.max, pair => pair[0] )
      if(!replacementWire) {
        error = { type: 'error', msg: "Profile was lost" }
        return wire
      }
      const [count, newWire] = replacementWire
      console.log('update profile', count, wireIds.size)
      if(count != wireIds.size) wasRepairNeeded = true
      return newWire
    })
    if(error) return error
    if(wasRepairNeeded) return { type: 'warning', msg: "Profile has been repaired" }
  }

  clone() {
    const clone = new Profile(this.sketch, [...this.rings])
    clone.id = this.id
    return clone
  }
}


// BREP

export class Shape {
  isSame(other) {
    return (this.id && this.id == other.id) || this.geom().IsSame(other.geom())
  }

  area() {
    if(!this.geom) return 0
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.SurfaceProperties_1(this.geom(), gprops, false, false)
    return gprops.Mass() // Mass really corresponds to surface area here
  }

  center() {
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.SurfaceProperties_1(this.geom(), gprops, false, false)
    return vecFromOc(gprops.CentreOfMass())
  }

  transform(workplane) {
    const transformed = transformGeometry(this.geom(), workplane)
    this.geom = () => transformed
  }

  collectShapes(type) {
    const geom = this.geom()

    const constructors = {
      edge: Edge,
      face: Face,
      solid: Solid,
    }

    new window.oc.oc.BRepMesh_IncrementalMesh_2(
      geom,
      0.4, // linear deflection
      false,
      0.2, // angular deflection
      false
    )

    return collectShapes(geom, type)
      .map(item => new constructors[type](this, item) )
  }

  tesselate() {
    const location = new window.oc.oc.TopLoc_Location_1()
    const triangulation = window.oc.oc.BRep_Tool.Triangulation(this.geom(), location, 0).get()
    triangulation.ComputeNormals()
    let positions = []
    let normals = []
    arrayRange(1, triangulation.NbTriangles()).forEach(i => {
      const triangle = triangulation.Triangle(i)
      const pos = arrayRange(1, 3).flatMap(j => coords(triangulation.Node(triangle.Value(j))) )
      const norm = arrayRange(1, 3).flatMap(j => coords(triangulation.Normal_1(triangle.Value(j))) )
      positions = positions.concat(pos)
      normals = normals.concat(norm)
    })
    return {
      positions,
      normals,
    }
  }
}


export class Volumetric extends Shape {
  static repair(shape) {

    // const purge = new window.oc.oc.TopOpeBRepTool_FuseEdges(shape, true)
    // purge.Perform()
    // shape = purge.Shape()

    const unify = new window.oc.oc.ShapeUpgrade_UnifySameDomain_2(shape, true, true, false)
    unify.Build()
    shape = unify.Shape()

    const fix = new window.oc.oc.ShapeFix_Shape_2(shape)
    fix.Perform(new window.oc.oc.Message_ProgressRange_1())
    shape = fix.Shape()

    // shape = new window.oc.oc.ShapeUpgrade_ShellSewing().ApplySewing(shape, 0.01)

    // console.log('open', window.oc.oc.BOPTools_AlgoTools.IsOpenShell(shell))
    // const explorer = new window.oc.oc.TopExp_Explorer_2(shape, window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHELL, window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHAPE)
    // while(explorer.More()) {
    //   const shell = window.oc.oc.TopoDS.Shell_1(explorer.Current())
    //   window.oc.oc.BOPTools_AlgoTools.OrientFacesOnShell(shell)
    //   explorer.Next()
    // }
    return shape
  }

  validate() {
    const analyzer = new window.oc.oc.BRepCheck_Analyzer(this.geom(), true, false)
    return analyzer.IsValid_2()
  }

  repair() {
    if(!this.geom) return
    const repaired = Volumetric.repair(this.geom())
    this.geom = () => repaired
    return this.validate()
  }

  volume() {
    if(!this.geom) return 0
    const gprops = new window.oc.oc.GProp_GProps_1() //XXX use GProp_GProps_2 to supply point close to center for better accuracy
    window.oc.oc.BRepGProp.VolumeProperties_1(this.geom(), gprops, false, false, false)
    return gprops.Mass()
  }

  center() {
    if(!this.geom) return
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.VolumeProperties_1(this.geom(), gprops, false, false, false)
    return gprops.CentreOfMass()
  }
}


export class Face extends Shape {
  constructor(solid, geom) {
    super()
    this.solid = solid
    this.geom = () => geom
    // this.id = crypto.randomUUID()
  }

  planarReference() {
    if(!this.getPlane()) throw "Cannot provide plane from non-planar face"
    return new PlanarReference(new FaceReference(this))
  }

  faceReference() {
    return new FaceReference(this)
  }

  getPlane() {
    const surface = window.oc.oc.BRep_Tool.Surface_2(this.geom())
    const isPlanar = new window.oc.oc.GeomLib_IsPlanarSurface(surface, 1.0e-7) //XXX Surface->IsKind(STANDARD_TYPE(Geom_Plane)) could be faster
    if(!isPlanar.IsPlanar()) return
    const plane = isPlanar.Plan()
    return matrixFromOcPln(plane)
  }

  normal(u, v, useCurvature) { //XXX use BOPTools_AlgoTools3D::GetNormalToSurface or BOPTools_AlgoTools3D::GetNormalToFaceOnEdge
    const geom = this.geom()
    const uMin = { current: 0 }
    const uMax = { current: 0 }
    const vMin = { current: 0 }
    const vMax = { current: 0 }
    window.oc.oc.BRepTools.UVBounds_1(geom.get(), uMin, uMax, vMin, vMax)
    u = uMin.current + (uMax.current - uMin.current) * u
    v = vMin.current + (vMax.current - vMin.current) * v
    const surface = window.oc.oc.BRep_Tool.Surface_2(geom)
    const props = new window.oc.oc.GeomLProp_SLProps_1(surface, u, v, useCurvature ? 2 : 1, 0.01)
    return vecFromOc(props.Normal())
  }
}


export class Edge extends Shape {
  constructor(solid, geom) {
    super()
    this.solid = solid
    this.geom = () => geom
    // this.id = crypto.randomUUID()
  }

  axialReference() {
    if(!this.getAxis()) throw "Cannot provide axis from non-linear edge"
    return new AxialReference(this.edgeReference())
  }

  edgeReference() {
    return new EdgeReference(this)
  }

  getAxis() {
    // BRep_Tool.Curve()
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    if(curve.GetType() != window.oc.oc.GeomAbs_CurveType.GeomAbs_Line) return
    const line = curve.Line()
    const dir = vecFromOc(line.Direction()).normalize()
    const loc = vecFromOc(line.Location())
    return rotationFromNormal(dir).setPosition(loc)
  }

  center() {
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    const middle = (curve.FirstParameter() + curve.LastParameter()) / 2.0
    return vecFromOc(curve.Value(middle))
  }

  vertices() {
    return collectShapes(this.geom(), 'vertex').map(vertex => vecFromOc(window.oc.oc.BRep_Tool.Pnt(vertex)) )
  }

  tesselate() {
    // const location = new window.oc.oc.TopLoc_Location_1()
    // const poly = window.oc.oc.BRep_Tool.Polygon3D(this.geom(), location).get()
    // const nodes = poly.Nodes()
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    return tesselateCurve(curve)
  }
}


export class Solid extends Volumetric {
  constructor(compound, geom) {
    super()
    this.compound = compound
    // geom = Solid.repair(geom)
    this.geom = () => geom
    // this.id = crypto.randomUUID()
  }

  typename() { return 'Solid' }

  faces() {
    const faces = this.collectShapes('face')
    faces.forEach((face, i) => face.id = this.id + '/face/' + i )
    return faces
  }

  edges() {
    const edges = this.collectShapes('edge')
    edges.forEach((edge, i) => edge.id = this.id + '/edge/' + i )
    return edges
  }
}


export class Compound extends Volumetric {
  constructor(componentId, geom) {
    super()
    this.componentId = componentId
    if(geom) this.geom = () => geom
    this.id = crypto.randomUUID()
  }

  solids() {
    if(!this.geom) return []
    this.cachedSolids ||= this.collectShapes('solid')
    this.cachedSolids.forEach((solid, i) => solid.id = this.id + '/solid/' + i )
    return this.cachedSolids
  }

  clone(geom) {
    const clone = new Compound(this.componentId, geom || (this.geom && this.geom()))
    clone.id = this.id
    return clone
  }

  boolean(other, op) {
    if(!other.geom) throw { type: 'error', msg: "Tool body has no volume" }

    if(!this.geom) return this.clone(other.geom())

    const ops = {
      join: window.oc.oc.BRepAlgoAPI_Fuse_3,
      cut: window.oc.oc.BRepAlgoAPI_Cut_3,
    }

    const result = new ops[op](this.geom(), other.geom(), new window.oc.oc.Message_ProgressRange_1())
    // return this.clone(Solid.repair(result.Shape()))
    return this.clone(result.Shape())
  }

  fillet(edges, radius) {
    const fillet = new window.oc.oc.BRepFilletAPI_MakeFillet(this.geom(), window.oc.oc.ChFi3d_FilletShape.ChFi3d_Rational)
    edges.forEach(edge => {
      fillet.Add_2(radius, edge.geom())
    })
    try {
      fillet.Build(new window.oc.oc.Message_ProgressRange_1())
      if(!fillet.IsDone()) throw null
      // return this.clone(Solid.repair(fillet.Shape()))
      return this.clone(fillet.Shape())
    } catch(_) {
      throw { type: 'error', msg: "Fillet could not be built" }
    }
  }

  offset(openFaces, distance) {
    // Build face list
    const faces = new window.oc.oc.TopTools_ListOfShape_1()
    openFaces.forEach(face => faces.Append_1(face.geom()) )
    // Offset each affected solid individually
    const solids = this.solids().filter(solid => solid.faces().some(face => openFaces.some(of => of.id == face.id ) ) )
    const substitution = new window.oc.oc.BRepTools_ReShape()
    solids.forEach(solid => {
      const thicken = new window.oc.oc.BRepOffsetAPI_MakeThickSolid()
      thicken.MakeThickSolidByJoin(
        solid.geom(),
        faces,
        distance,
        1.0e-6, // window.oc.oc.Precision.Confusion()
        window.oc.oc.BRepOffset_Mode.BRepOffset_Skin,
        false,
        false,
        window.oc.oc.GeomAbs_JoinType.GeomAbs_Arc,
        false,
        new window.oc.oc.Message_ProgressRange_1()
      )
      if(!thicken.IsDone()) throw { type: 'error', msg: "Offset could not be built" }
      substitution.Replace(solid.geom(), thicken.Shape())
    })
    // Replace updated solids in compound
    const compound = substitution.Apply(this.geom(), window.oc.oc.TopAbs_ShapeEnum.TopAbs_SOLID)
    const out = this.clone(compound)
    if(!out.repair()) throw { type: 'error', msg: "Could not close shell" }
    return out
  }
}


// References

export class Reference {
  constructor(item) {
    this.item = item
  }

  getItem() {
    return this.item
  }

  update(_tree) {}

  clone() {
    return new this.constructor(this.item)
  }
}

export class FaceReference extends Reference {
  update(tree) {
    const comp = tree.findChild(this.item.solid.compound.componentId)
    const solid = comp.compound.solids().find(solid => solid.id == this.item.solid.id )
    const face = solid.faces().find(face => face.isSame(this.item) )
    if(!face) return { type: 'error', msg: "Face reference was lost" }
    this.item = face
  }
}

export class EdgeReference extends Reference {
  update(tree) {
    const comp = tree.findChild(this.item.solid.compound.componentId)
    const solid = comp.compound.solids().find(solid => solid.id == this.item.solid.id )
    const edge = solid.edges().find(edge => edge.isSame(this.item) )
    if(!edge) return { type: 'error', msg: "Edge reference was lost" }
    this.item = edge
  }
}

export class CurveReference extends Reference {
  update(_tree) {
    if(!this.item.projection) return
    this.item = this.item.projection.geometry()
  }
}

export class ProfileReference extends Reference {
  update(_tree) {
    return this.item.update()
  }

  clone() {
    return new ProfileReference(this.item.clone())
  }
}

export class PlanarReference extends Reference {
  getReal() {
    if(this.item instanceof PlaneHelper) {
      return this.item
    } else if(this.item instanceof FaceReference) {
      return this.item.getItem()
    }
  }

  getItem() {
    const item = this.getReal()
    return item && item.getPlane()
  }

  update(tree) {
    if(this.item instanceof FaceReference) {
      return this.item.update(tree)
    }
  }

  clone() {
    return new PlanarReference(this.item instanceof FaceReference ? this.item.clone() : this.item)
  }
}

export class AxialReference extends Reference {
  getReal() {
    if(this.item instanceof AxisHelper) {
      return this.item
    } else {
      return this.item.getItem()
    }
  }

  getItem() {
    const item = this.getReal()
    return item && item.getAxis()
  }

  update(tree) {
    if(!(this.item instanceof AxisHelper)) {
      return this.item.update(tree)
    }
  }

  clone() {
    return new AxialReference(this.item instanceof AxisHelper ? this.item : this.item.clone())
  }
}


// Helpers

export class ConstructionHelper {
  constructor() {
    this.id = crypto.randomUUID()
  }

  center() {
    return new THREE.Vector3().setFromMatrixPosition(this.transform)
  }
}

export class PlaneHelper extends ConstructionHelper {
  constructor(plane) {
    super()
    this.transform = plane
  }

  planarReference() {
    return new PlanarReference(this)
  }

  getPlane() {
    return this.transform
  }
}

export class AxisHelper extends ConstructionHelper {
  constructor(axis) {
    super()
    this.transform = axis
  }

  axialReference() {
    return new AxialReference(this)
  }

  getAxis() {
    return this.transform
  }
}


// Timeline

export class Timeline {
  constructor() {
    const baseComp = new Component(null, "00000000-0000-0000-0000-000000000000")
    baseComp.creator = {
      title: "Main Assembly",
      sectionViews: [],
      parameters: [],
      exportConfigs: [],
      itemsHidden: {},
      color: 'purple',
    }
    const cache = {
      faces: [],
      edges: [],
      regions: [],
      curves: [],
      helpers: [],
      dimensions: [],
    }
    baseComp.creator.cache = () => cache

    this.features = []
    this.cache = [baseComp]
    this.marker = 0
    this.last_change_index = 0
    this.last_eval_index = 0
    // removal_modifications: Vec<CompRef>,
  }

  tree(at = this.marker) {
    if(at instanceof Feature) at = this.features.indexOf(at) + 1
    // Return last state if current feature hasn't been executed yet
    return this.cache[at]// || this.cache[this.marker - 1]
  }

  moveMarkerToFeature(feature) {
    this.marker = this.features.indexOf(feature) + 1
  }

  isCurrentFeature(feature) {
    return this.marker == this.features.indexOf(feature) + 1
  }

  previousFeature(feature) {
    let i = this.features.indexOf(feature)
    return this.features[i - 1]
  }

  insertFeature(feature) {
    this.features.splice(this.marker, 0, feature)
    this.last_change_index = Math.min(this.last_change_index, this.marker)
    // console.log('insertFeature', this.last_change_index, this.marker)
    this.marker++
    this.evaluate()
  }

  invalidateFeature(feature) {
    this.last_change_index = this.features.indexOf(feature)
    // console.log('invalidating -> last_change_index', this.last_change_index)
  }

  // repairFeature(feature) {
  //   const index = this.features.indexOf(feature)
  //   const comp = this.cache[index]
  //   feature.repair(comp)
  //   this.invalidateFeature(feature)
  // }

  removeFeature(feature) {
    // this.removal_modifications.append(
    //   &mut feature.borrow().feature_type.as_feature().modified_components()
    // );
    let index = this.features.indexOf(feature)
    this.features = this.features.filter(f => f != feature )
    if(this.marker > index) {
      this.marker -= 1
      this.last_eval_index -= 1
    }
    // this.last_change_index = Math.max(0, Math.min(this.last_change_index, index))
    this.last_change_index = Math.min(this.last_change_index, index)
    // console.log('removeFeature', this.last_change_index, this.features)
  }

  evaluate() {
    const last_change = this.last_change_index
    console.log('evaluate', last_change, this.marker)
    this.regenerate(Math.min(last_change, this.marker), this.marker)
    const [from, to] = ordered(this.last_eval_index, this.marker)
    this.last_eval_index = this.marker
    return this.componentsModified(Math.min(from, last_change), to)
  }

  regenerate(from, to) {
    // this.cache.resize(this.features.len() + 1, Component::default());
    // console.log('before regenerate', from, to, this.cache)
    for(let i = from; i < to; i++) {
      const feature = this.features[i]
      let newComp = this.cache[i].deepClone()
      let j = i + 1
      // console.log('updating cache at with', j, newComp)
      this.cache[j] = newComp
      feature.execute(newComp)
      if(feature.error && feature.error.type == 'error') {
        this.cache[j] = this.cache[i].deepClone()
      } else {
        // let repair_error = feature.modified_components()
        //   .find_map(|id| newComp.find_child_mut(id).unwrap().compound.repair().err() )
        //   .map(|error| FeatureError::Error(error) );
        // if repair_error.is_some() {
        //   feature.error = repair_error;
        //   this.cache[j] = this.cache[i].deep_clone()
        // } else {
          // console.log('updating cache at with', j, newComp)
          // this.cache[j] = newComp
        // }
      }
      this.last_change_index = j
    }
    // console.log('after regenerate', from, to, this.cache)
  }

  componentsModified(from, to) {
    // Find unique ids of modified components in given range
    // for(let i = from; i < to; i++) {
    //   const feature = this.features[i]
    // }
    let compIds = [...new Set(
      arrayRange(from, to - 1)
        .map(i => this.features[i].modifiedComponents() )
        // comp_ids.append(&mut this.removal_modifications);
        .flat()
    )]
    // Filter children whose parents are already part of the set
    const comps = compIds.map(id => this.cache[to].findChild(id) )
    // console.log(compIds, this.cache, comps)
    compIds = compIds.filter(id => !comps.some(comp => hasChild(comp, id) ) )
    return compIds
  }

  getFutureChildIds(compId) {
    const tree = this.finalTree()
    // const comp = this.getFutureComp(compId, tree)
    const comp = tree.findChild(compId)
    // return this.getChildIds(comp)
    return comp.getChildIds()
  }

  // getFutureComp(id, tree) {
  //   if(id == tree.id) return tree
  //   for(const child of tree.children) {
  //     const self = this.getFutureComp(id, child)
  //     if(self) return self
  //   }
  // }

  // getChildIds(comp) {
  //   let ids = [comp.id]
  //   for(const child of comp.children) {
  //     ids = ids.concat(this.getChildIds(child))
  //   }
  //   return ids
  // }

  finalTree() {
    let tree = new Component(null, "00000000-0000-0000-0000-000000000000")
    tree.creator = { color: 'purple' }
    this.features.forEach(feature => {
      if(feature instanceof CreateComponentFeature) {
        let parent = tree.findChild(feature.parent())
        const child = new Component(parent, feature.component.id)
        child.creator = feature
        parent.children.push(child)
      }
    })
    return tree
  }
}


function coords(ocVec) {
  return [ocVec.X(), ocVec.Y(), (ocVec.Z && ocVec.Z()) || 0.0]
}

function vecFromOc(ocVec) {
  return new THREE.Vector3().fromArray(coords(ocVec))
}

function ocPnt2dFromVec(vec) {
  return new window.oc.oc.gp_Pnt2d_3(vec.x, vec.y)
}

function ocPntFromVec(vec) {
  return new window.oc.oc.gp_Pnt_3(vec.x, vec.y, vec.z)
}

function ocDirFromVec(vec) {
  return new window.oc.oc.gp_Dir_4(vec.x, vec.y, vec.z)
}

function ocDir2dFromVec(vec) {
  return new window.oc.oc.gp_Dir2d_4(vec.x, vec.y)
}

function ocVecFromVec(vec) {
  return new window.oc.oc.gp_Vec_4(vec.x, vec.y, vec.z)
}

function ocCirc2dFromVec(center, radius) {
  center = ocPnt2dFromVec(center)
  const v = new window.oc.oc.gp_Dir2d_4(0.0, 1.0)
  const axis = new window.oc.oc.gp_Ax2d_2(center, v)
  return new window.oc.oc.gp_Circ2d_2(axis, radius, true)
}

function ocAx1FromMatrix(m, constructor=window.oc.oc.gp_Ax1_2) {
  const pos = new THREE.Vector3().setFromMatrixPosition(m)
  const rot = new THREE.Quaternion().setFromRotationMatrix(m)
  const axDir = new THREE.Vector3(0,0,1).applyQuaternion(rot)
  return new window.oc.oc.gp_Ax1_2(ocPntFromVec(pos), ocDirFromVec(axDir))
}

function ocAx3FromMatrix(m, constructor=window.oc.oc.gp_Ax1_2) {
  const pos = new THREE.Vector3().setFromMatrixPosition(m)
  const rot = new THREE.Quaternion().setFromRotationMatrix(m)
  const axDir = new THREE.Vector3(0,0,1).applyQuaternion(rot)
  const xDir = new THREE.Vector3(1,0,0).applyQuaternion(rot)
  return new window.oc.oc.gp_Ax3_3(ocPntFromVec(pos), ocDirFromVec(axDir), ocDirFromVec(xDir))
}

function matrixFromOcPln(pln) {
  const x = vecFromOc(pln.XAxis().Direction())
  const y = vecFromOc(pln.YAxis().Direction())
  const z = vecFromOc(pln.Axis().Direction())
  return new THREE.Matrix4()
    .makeBasis(x, y, z)
    .setPosition(vecFromOc(pln.Axis().Location()))
}

function ocPlnFromMatrix(m) {
  return new window.oc.oc.gp_Pln_2(ocAx3FromMatrix(m))
}

function tesselateCurveFixed(geom, steps) {
  const start = geom.FirstParameter()
  const range = geom.LastParameter() - start

  const vertices = arrayRange(0, steps - 1).map(i => {
    const u = start + i / (steps - 1) * range
    const p = geom.Value(u)
    return coords(p)
  })
  return vertices
}

function tesselateCurve(geom) {
  const deflection = new window.oc.oc.GCPnts_TangentialDeflection_2(geom,
    0.1, // curvature deflection
    0.1, // angular deflection
    2, // min points
    1.0e-9, 1.0e-7
  )
  return arrayRange(1, deflection.NbPoints()).map(i => coords(deflection.Value(i)) )
}

function transformGeometry(geom, workplane) {
  const pos = new THREE.Vector3().setFromMatrixPosition(workplane)
  const rot = new THREE.Quaternion().setFromRotationMatrix(workplane)

  const trans = new window.oc.oc.gp_Trsf_1()
  const quat = new window.oc.oc.gp_Quaternion_2(rot.x, rot.y, rot.z, rot.w)
  trans.SetRotationPart(quat)
  trans.SetTranslationPart(ocVecFromVec(pos))

  return new window.oc.oc.BRepBuilderAPI_Transform_2(geom, trans, true).Shape()
}

function collectShapes(geom, type) {
  const enums = {
    vertex: window.oc.oc.TopAbs_ShapeEnum.TopAbs_VERTEX,
    edge: window.oc.oc.TopAbs_ShapeEnum.TopAbs_EDGE,
    face: window.oc.oc.TopAbs_ShapeEnum.TopAbs_FACE,
    solid: window.oc.oc.TopAbs_ShapeEnum.TopAbs_SOLID,
  }
  const converters = {
    vertex: window.oc.oc.TopoDS.Vertex_1,
    edge: window.oc.oc.TopoDS.Edge_1,
    face: window.oc.oc.TopoDS.Face_1,
    solid: window.oc.oc.TopoDS.Solid_1,
  }

  const map = new window.oc.oc.TopTools_IndexedMapOfShape_1()
  window.oc.oc.TopExp.MapShapes_1(geom, enums[type], map)

  return arrayRange(1, map.Extent())
    .map(i => map.FindKey(i) )
    .map(shape => converters[type](shape) )
}

function cloneShape(shape) {
  return new window.oc.oc.BRepBuilderAPI_Copy_2(shape, true, false).Shape() // copyGeom, copyMesh
}

function ordered(a, b) {
  return a < b ? [a, b] : [b, a]
}

function hasChild(comp, id) {
  const child = comp.findChild(id)
  return child && !child == comp
}

export function rotationFromNormal(normal) {
  let up = THREE.Object3D.DEFAULT_UP
  let xAxis
  if(Math.abs(normal.dot(up)) > 0.9999) {
    xAxis = new THREE.Vector3(1, 0, 0)
  } else {
    xAxis = new THREE.Vector3().crossVectors(up, normal).normalize()
  }
  const yAxis = new THREE.Vector3().crossVectors(normal, xAxis)
  const rot = new THREE.Matrix4().makeBasis(xAxis, yAxis, normal)
  return rot
  // return new THREE.Quaternion().setFromRotationMatrix(rot)
  // let radians = Math.acos(normal.dot(up))
  // return new THREE.Quaternion().setFromAxisAngle(xAxis, radians)
}

function cross2d(a, b) {
  return a.x * b.y - a.y * b.x
}

// Check if two line segments turn clockwise
// Returns values < 0 when clockwise, > 0 when anti-clockwise and 0 when segments are colinear
function clockwise(p1, p2, p3) {
  let v1 = p2.clone().sub(p1).normalize()
  let v2 = p3.clone().sub(p2).normalize()
  let v3 = p3.clone().sub(p1).normalize()
  // Cross product changes sign with clockwiseness,
  // but doesn't show if angle is steeper or shallower than 90 degrees
  // (symmetric between front and back)
  let cross = cross2d(v1, v3)
  // Dot product is "left/right" symmetric,
  // but negative for steep angles and positive for shallow angles
  let dot = Math.abs(v1.dot(v2) - 1.0) / 2.0 // Range shallow to steep => 0 -> 1
  return dot * Math.sign(cross)
}

function isClockwise(closedPolyline) {
  return signedPolygonArea(closedPolyline) < 0.0
}

function polygon_area(closedPolyline) {
  return signedPolygonArea(closedPolyline).abs()
}

function signedPolygonArea(closedPolyline) {
  let signedArea = 0.0
  let len = closedPolyline.length
  arrayRange(0, len - 1).forEach(i => {
    let j = (i + 1) % len
    let p = closedPolyline[i]
    let nextP = closedPolyline[j]
    signedArea += p.x * nextP.y - nextP.x * p.y
  })
  return signedArea / 2.0
}

export const EPSILON = 0.000001

Number.prototype.almost = function(other) {
  return Math.abs(this - other) < EPSILON
}

THREE.Vector3.prototype.almost = function(other) {
  return Math.abs(this.x - other.x) < EPSILON &&
         Math.abs(this.y - other.y) < EPSILON &&
         Math.abs(this.z - other.z) < EPSILON
}

Array.prototype.minMaxBy = function(comparator, lambda) {
  var lambdaFn;
  if (typeof(lambda) === "function") {
      lambdaFn = lambda;
  } else {
      lambdaFn = function(x){
          return x[lambda];
      }
  }
  var mapped = this.map(lambdaFn);
  var minValue = comparator.apply(Math, mapped);
  return this[mapped.indexOf(minValue)];
}
