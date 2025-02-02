import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { Line, Circle, Arc, Spline, SketchElement } from './geom2d.js'
import { Wire, Profile } from './geom3d.js'
import { EPSILON, clockwise, ocAx3FromMatrix, ocPlnFromMatrix } from './utils.js'
import { CurveReference, EdgeReference } from './references.js'


export class Sketch {
  constructor() {
    this.id = makeID()
    this.elements = []
    this.constraints = []
    this.projections = []
    this.workplane = new THREE.Matrix4()
    // this.componentId = componentId
  }

  typename() { return 'Sketch' }

  add(elem) {
    if(!this.elements.includes(elem)) this.elements.push(elem)
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
    const cutElements = elements.flatMap(elem => elem.split(elements) )
    const wires = this.getWires(cutElements, includeOuter)
    const profiles = this.buildProfiles(comp, wires)

    return profiles
  }

  removeEmpties(elements) {
    return elements.filter(curve => !curve.isReference && !curve.length().almost(0.0) )
  }

  getWires(cutElements, includeOuter) {
    const circles = cutElements.filter(elem => elem.isClosed() )
    let others = cutElements.filter(elem => !elem.isClosed() )
    others = this.removeDanglingSegments(others)
    let islands = this.buildIslands(others)
    let wires = islands.flatMap(island => this.buildWiresFromIsland(island, includeOuter) )
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
    island.forEach(startElem => {
      startElem.endpoints().forEach(point => {
        const path = []
        this.buildLoop(
          point,
          startElem,
          path,
          island,
          usedForward,
          usedBackward,
        )
        if(path.length >= 2) wires.push(new Wire(path))
      })
    })
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
    // Traverse edges only once in every direction
    let startElemId = startElem.id
    if(startPoint.almost(startElem.endpoints()[0])) {
      if(usedForward.has(startElemId)) return
      usedForward.add(startElemId)
    } else {
      if(usedBackward.has(startElemId)) return
      usedBackward.add(startElemId)
    }
    // Add startElem to path
    path.push(startElem)
    // Terminate loop
    let endPoint = startElem.otherBound(startPoint)
    if(path[0] != startElem && path[0].endpoints().some(p => p.almost(endPoint) )) return
    // Find connected segments
    let connectedElems = allElements.filter(otherElem => {
      let [otherStart, otherEnd] = otherElem.endpoints()
      return (endPoint.almost(otherStart) || endPoint.almost(otherEnd)) &&
        otherElem.id != startElemId
    })
    if(connectedElems.length) {
      // Sort connected segments in clockwise order
      connectedElems.sort((a, b) => {
        let finalPointA = a.sample(0.5)
        let finalPointB = b.sample(0.5)
        return clockwise(startPoint, endPoint, finalPointA) - clockwise(startPoint, endPoint, finalPointB)
      })
      // Follow the leftmost segment to complete loop in anti-clockwise order
      let nextElem = connectedElems[0]
      if(path[0].id != nextElem.id) {
        this.buildLoop(
          endPoint,
          nextElem,
          path,
          allElements,
          usedForward,
          usedBackward,
        )
      }
    }
  }

  buildIslands(elements) {
    let unusedElements = [...elements]
    let islands = []
    while(unusedElements.length) {
      let startElem = unusedElements.pop()
      let island = []
      this.buildIsland(startElem, island, unusedElements)
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
        const p1 = { id: `${id++}`, type: 'point', x: elem.points[0].x, y: elem.points[0].y, fixed: elem.projection }
        const p2 = { id: `${id++}`, type: 'point', x: elem.points[1].x, y: elem.points[1].y, fixed: elem.projection }
        const line = { id: `${id++}`, type: 'line', p1_id: p1.id, p2_id: p2.id }
        primitives = [p1, p2, line]

      } else if(elem instanceof Circle) {
        const center = { id: `${id++}`, type: 'point', x: elem._center.x, y: elem._center.y, fixed: elem.projection }
        const circle = { id: `${id++}`, type: 'circle', c_id: center.id, radius: elem.radius }
        primitives = [center, circle]

      } else if(elem instanceof Arc) {
        const center = { id: `${id++}`, type: 'point', x: elem._center.x, y: elem._center.y, fixed: elem.projection }
        const endpoints = elem.endpoints()
        const start =  { id: `${id++}`, type: 'point', x: endpoints[0].x, y: endpoints[0].y, fixed: elem.projection }
        const end =    { id: `${id++}`, type: 'point', x: endpoints[1].x, y: endpoints[1].y, fixed: elem.projection }
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

      } else if(elem instanceof Spline) {
        const p1 = { id: `${id++}`, type: 'point', x: elem.points[0].x, y: elem.points[0].y, fixed: elem.projection }
        const p2 = { id: `${id++}`, type: 'point', x: elem.points.slice(-1)[0].x, y: elem.points.slice(-1)[0].y, fixed: elem.projection }
        primitives = [p1, p2]
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

      } else if(elem instanceof Spline) {
        const [p1, p2] = idMap[elem.id]
        const handles = elem.handles()
        handles[0] = vecFromPrim(p1)
        handles[handles.length - 1] = vecFromPrim(p2)
        elem.setHandles(handles)
      }
    })
  }

  dump() {
    return {
      id: this.id,
      elements: this.elements,
      constraints: this.constraints,
      projections: this.projections,
    }
  }

  static undump(dump, context) {
    const sketch = new Sketch()
    Object.assign(sketch, dump)
    sketch.elements.forEach(elem => elem.sketch = sketch )
    context.sketches[sketch.id] = sketch
    return sketch
  }
}
Serialize.register(Sketch, 'Sketch')


export class Projection {
  constructor(edgeOrRef) {
    this.edgeRef = edgeOrRef instanceof EdgeReference ? edgeOrRef : new EdgeReference(edgeOrRef)
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

  geometry() { return this.output }

  static undump(dump) { return new Projection(dump.edgeRef) }
}
Serialize.register(Projection, 'Projection')


export class ElemRef {
  constructor(curveOrRef, index) {
    this.curveRef = curveOrRef instanceof CurveReference ? curveOrRef : new CurveReference(curveOrRef)
    this.index = index
  }

  update(tree) {
    this.curveRef.update(tree)
  }

  curve() {
    return this.curveRef.getItem()
  }

  isPoint() { return this.index !== undefined }

  static undump(dump) { return new ElemRef(dump.curveRef, dump.index) }
}
Serialize.register(ElemRef, 'ElemRef')


export class Constraint {
  constructor(...items) {
    this.items = items.map(item => item instanceof ElemRef ? item : new ElemRef(item) )
  }

  update(tree) {
    this.items.forEach(item => item.update(tree) )
  }

  static undump(dump) { return new Constraint(...dump.items) }
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
Serialize.register(CoincidentConstraint, 'CoincidentConstraint')

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
