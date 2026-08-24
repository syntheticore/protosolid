import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { Line, Circle, Arc, Spline, SketchElement, SketchPoint, supportsPointOnCurveConstraint } from './geom2d.js'
import { Wire, Profile, Edge } from './geom3d.js'
import { AxisHelper, PointHelper } from './helpers.js'
import { EPSILON, cross2d, ocAx3FromMatrix, ocPlnFromMatrix, ocPntFromVec, transformGeometry } from './utils.js'
import { Reference, CurveReference, EdgeReference, HelperReference, SketchOriginReference } from './references.js'
import { relativeComponentTransform } from './assembly.js'
import Expression from './expression.js'

function sameCurve(left, right) {
  if(left === right) return true
  if(!left || !right) return false
  if(left.projection || right.projection) return left.projection?.id === right.projection?.id
  return left.id === right.id
}

function sameElemRef(left, right) {
  const leftRef = left.curveRef
  const rightRef = right.curveRef
  return left.index === right.index && (
    sameCurve(left.curve(), right.curve()) ||
    (leftRef.constructor === rightRef.constructor &&
      leftRef.componentId === rightRef.componentId &&
      leftRef.sketchId === rightRef.sketchId &&
      leftRef.isProjection === rightRef.isProjection &&
      leftRef.itemId === rightRef.itemId)
  )
}

function sameConstraint(left, right) {
  if(left instanceof Dimension || right instanceof Dimension) return false
  if(left.constructor !== right.constructor || left.items.length != right.items.length) return false
  if(left instanceof HorVertConstraint && left.isVertical != right.isVertical) return false
  const unmatched = [...right.items]
  return left.items.every(item => {
    const index = unmatched.findIndex(other => sameElemRef(item, other))
    if(index == -1) return false
    unmatched.splice(index, 1)
    return true
  })
}


export class SketchOrigin {
  constructor(sketch) {
    this.sketch = sketch
    this.id = sketch.id + '/origin'
  }

  typename() { return 'Sketch Origin' }
  center() { return new THREE.Vector3() }
  handles() { return [this.center()] }
  endpoints() { return this.handles() }
}


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
    elem.fullyConstrained = false
  }

  addConstraint(constraint) {
    const existing = this.constraints.find(other => sameConstraint(other, constraint))
    if(existing) return existing
    if(constraint instanceof CoincidentConstraint && constraint.items.length == 2) {
      const [left, right] = constraint.items
      const leftCurve = left.curve()
      const rightCurve = right.curve()
      if(leftCurve && rightCurve && this.coincidentPointsConnected(
        leftCurve, left.index, rightCurve, right.index
      )) return this.constraints.find(other =>
        other instanceof CoincidentConstraint && other.items.some(item =>
          item.curve() == leftCurve && item.index == left.index
        )
      ) || constraint
    }
    constraint.sketch = this
    this.constraints.push(constraint)
    return constraint
  }

  addProjection(projection) {
    projection.sketch = this
    this.projections.push(projection)
  }

  origin() {
    if(!this._origin) this._origin = new SketchOrigin(this)
    return this._origin
  }

  originPoint() {
    return this.origin().center()
  }

  remove(elem) {
    this.elements.forEach(element => element.fullyConstrained = false)
    if(elem instanceof SketchElement) {
      this.elements = this.elements.filter(e => e != elem )
      this.constraints = this.constraints.filter(c => !c.items.some(item => item.curve() == elem ) )
      if(elem.projection) this.projections = this.projections.filter(p => p != elem.projection )

    } else if(elem instanceof Constraint) {
      this.constraints = this.constraints.filter(c => c != elem )
    }
  }

  findOrCreateIntersectionPoint(curves, position, point) {
    const sameCurves = constraint => {
      if(!(constraint instanceof IntersectionConstraint)) return false
      const pointItem = constraint.items.find(item => item.index !== undefined)
      const curveItems = constraint.items.filter(item => item.index === undefined)
      const constrainedPoint = pointItem && pointItem.curve()
      return curveItems.length == curves.length && constrainedPoint &&
        curves.every(curve => curveItems.some(item => sameCurve(item.curve(), curve))) &&
        constrainedPoint.center().almost(position)
    }
    const existingConstraint = this.constraints.find(sameCurves)
    if(existingConstraint) return existingConstraint.items.find(item => item.index !== undefined).curve()

    point = point || new SketchPoint(position.clone())
    if(!this.elements.includes(point)) this.add(point)
    this.addConstraint(new IntersectionConstraint(
      new ElemRef(point, 0),
      ...curves,
    ))
    return point
  }

  dissolvePoint(point, elem, index) {
    if(point.constraints().some(constraint => constraint instanceof IntersectionConstraint)) return false
    const replacement = new ElemRef(elem, index)
    point.constraints().forEach(constraint => {
      constraint.items.forEach(item => {
        if(item.curve() != point) return
        item.curveRef = replacement.curveRef.clone()
        item.index = replacement.index
      })
    })
    this.constraints = this.constraints.filter(constraint => {
      if(!(constraint instanceof CoincidentConstraint)) return true
      return !(constraint.items.length == 2 &&
        constraint.items[0].curve() == constraint.items[1].curve() &&
        constraint.items[0].index == constraint.items[1].index)
    })
    this.constraints = this.constraints.filter((constraint, index, constraints) =>
      constraint instanceof Dimension || constraints.findIndex(other => sameConstraint(other, constraint)) == index
    )
    this.removeRedundantCoincidentConstraints()
    point.dissolvedTo = { elem, index }
    this.remove(point)
    return true
  }

  coincidentPointsConnected(elem, index, targetElem, targetIndex) {
    const pending = [{ elem, index }]
    const visited = []
    while(pending.length) {
      const point = pending.pop()
      if(visited.some(other => other.elem == point.elem && other.index == point.index)) continue
      if(point.elem == targetElem && point.index == targetIndex) return true
      visited.push(point)
      this.constraints.forEach(constraint => {
        if(!(constraint instanceof CoincidentConstraint) || !constraint.items.some(item =>
          item.curve() == point.elem && item.index == point.index
        )) return
        constraint.items.forEach(item => pending.push({ elem: item.curve(), index: item.index }))
      })
    }
    return false
  }

  removeRedundantCoincidentConstraints() {
    const constraints = this.constraints
    this.constraints = []
    constraints.forEach(constraint => {
      if(constraint instanceof CoincidentConstraint && constraint.items.length == 2) {
        const [left, right] = constraint.items
        const leftCurve = left.curve()
        const rightCurve = right.curve()
        if(leftCurve && rightCurve && this.coincidentPointsConnected(
          leftCurve, left.index, rightCurve, right.index
        )) return
      }
      this.constraints.push(constraint)
    })
  }

  replaceElement(elem, replacements) {
    const elementIndex = this.elements.indexOf(elem)
    if(elementIndex == -1 || !replacements.length) return []

    const oldHandles = elem.handles ? elem.handles().map(point => point.clone()) : []
    replacements.forEach(replacement => replacement.sketch = this)

    // Keep the original ID on the replacement that preserves the most old
    // handles. Besides making downstream curve references more stable, this
    // picks the intuitive survivor when trimming an end off a curve.
    const primary = replacements.minMaxBy(Math.max, replacement =>
      replacement.handles().filter(point => oldHandles.some(old => old.almost(point))).length
    )
    primary.id = elem.id
    replacements.filter(replacement => replacement != primary).forEach(replacement => replacement.id = makeID())

    const lostConstraints = new Set()
    this.constraints.forEach(constraint => constraint.items.forEach(item => {
      if(item.curve() != elem) return

      let replacement = primary
      let replacementIndex = item.index
      if(item.index !== undefined) {
        const oldPoint = oldHandles[item.index]
        const match = replacements
          .map(curve => ({ curve, index: curve.handles().findIndex(point => point.almost(oldPoint)) }))
          .find(candidate => candidate.index != -1)
        if(!match) {
          lostConstraints.add(constraint)
          return
        }
        replacement = match.curve
        replacementIndex = match.index
      }

      item.curveRef.item = replacement
      item.curveRef.itemId = replacement.id
      item.index = replacementIndex
    }))

    this.constraints = this.constraints.filter(constraint => !lostConstraints.has(constraint))
    this.elements.splice(elementIndex, 1, ...replacements)
    this.elements.forEach(element => element.fullyConstrained = false)
    elem.clear()
    return replacements
  }

  profiles(comp, includeOuter) {
    const elements = this.profileElements()
    const cutElements = elements.flatMap(elem => elem.split(elements) )
    const wires = this.getWires(cutElements, includeOuter)
    const profiles = this.buildProfiles(comp, wires)

    return profiles
  }

  profileElements() {
    const projections = this.projections
      .map(projection => projection.geometry())
      .filter(elem => elem instanceof SketchElement)
    return this.removeEmpties([...this.elements, ...projections])
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
        const closed = this.buildLoop(
          point,
          startElem,
          path,
          island,
          usedForward,
          usedBackward,
          point,
        )
        if(closed && path.length >= 2) wires.push(new Wire(path, point))
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
    loopStartPoint,
  ) {
    // Traverse edges only once in every direction
    let startElemId = startElem.id
    if(startPoint.almost(startElem.endpoints()[0])) {
      if(usedForward.has(startElemId)) return false
      usedForward.add(startElemId)
    } else {
      if(usedBackward.has(startElemId)) return false
      usedBackward.add(startElemId)
    }
    // Add startElem to path
    path.push(startElem)
    // Terminate loop
    let endPoint = startElem.otherBound(startPoint)
    if(path.length > 1 && endPoint.almost(loopStartPoint)) return true
    // Find connected segments
    let connectedElems = allElements.filter(otherElem => {
      let [otherStart, otherEnd] = otherElem.endpoints()
      return (endPoint.almost(otherStart) || endPoint.almost(otherEnd)) &&
        otherElem.id != startElemId
    })
    if(connectedElems.length) {
      const startEndpoints = startElem.endpoints()
      const incomingSampleParam = endPoint.almost(startEndpoints[1]) ? 1.0 - EPSILON : EPSILON
      const incoming = endPoint.clone().sub(startElem.sample(incomingSampleParam)).normalize()
      // Keep the face on the left by following the outgoing segment with the
      // largest signed turn from the incoming direction. Sampling close to the
      // shared endpoint is important for arcs: their midpoint can lie on the
      // opposite side of the incoming tangent and produce a non-planar walk.
      connectedElems.sort((a, b) => {
        const turn = elem => {
          const endpoints = elem.endpoints()
          const sampleParam = endPoint.almost(endpoints[0]) ? EPSILON : 1.0 - EPSILON
          const outgoing = elem.sample(sampleParam).sub(endPoint).normalize()
          return Math.atan2(cross2d(incoming, outgoing), incoming.dot(outgoing))
        }
        return turn(b) - turn(a)
      })
      // Follow the leftmost segment to complete loop in anti-clockwise order
      let nextElem = connectedElems[0]
      return this.buildLoop(
        endPoint,
        nextElem,
        path,
        allElements,
        usedForward,
        usedBackward,
        loopStartPoint,
      )
    }
    return false
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

  solve(tree, draggedHandle) {
    let id = 1
    const idMap = {}

    // Convert entire sketch to GCS format
    const projections = this.projections.map(p => p.update(tree) ).filter(Boolean)
    const primitives = this.elements.concat(projections).flatMap(elem => {
      let primitives

      if(elem instanceof SketchPoint) {
        primitives = [{ id: `${id++}`, type: 'point', x: elem.point.x, y: elem.point.y }]

      } else if(elem instanceof Line) {
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
        const angles = endpoints.map(point => Math.atan2(
          point.y - elem._center.y,
          point.x - elem._center.x,
        ))
        const start =  { id: `${id++}`, type: 'point', x: endpoints[0].x, y: endpoints[0].y, fixed: elem.projection }
        const end =    { id: `${id++}`, type: 'point', x: endpoints[1].x, y: endpoints[1].y, fixed: elem.projection }
        const arc = {
          id: `${id++}`,
          type: 'arc',
          c_id: center.id,
          radius: elem.radius,
          // A three-point OpenCascade arc can choose either direction for
          // its local circle axis. PlaneGCS uses Cartesian counter-clockwise
          // angles, so derive them from the actual endpoints rather than OC's
          // axis-relative curve parameters.
          start_angle: angles[0],
          end_angle: angles[1],
          start_id: start.id,
          end_id: end.id,
        }
        primitives = [start, end, center, arc]

      } else if(elem instanceof Spline) {
        const poles = elem.points.map(point => ({
          id: `${id++}`,
          type: 'point',
          x: point.x,
          y: point.y,
          fixed: !!elem.projection,
        }))
        const degree = Math.min(5, poles.length - 1)
        const [knots, mult] = Spline.clampedKnots(poles.length, degree)
        const spline = {
          id: `${id++}`,
          type: 'bspline',
          pole_ids: poles.map(pole => pole.id),
          weights: poles.map(() => 1.0),
          knots,
          mult,
          degree,
          periodic: false,
        }
        // A clamped spline's endpoint tangent is exactly the direction of
        // its first control polygon segment. Keep that derived line in the
        // solver so tangent constraints can use the stable parallel-line
        // primitive.
        const startTangent = {
          id: `${id++}`,
          type: 'line',
          p1_id: poles[0].id,
          p2_id: poles[1].id,
          splineTangent: 0,
        }
        const endTangent = {
          id: `${id++}`,
          type: 'line',
          p1_id: poles[poles.length - 2].id,
          p2_id: poles[poles.length - 1].id,
          splineTangent: 1,
        }
        primitives = [...poles, startTangent, endTangent, spline]

      } else if(elem instanceof ProjectedPoint) {
        primitives = [{ id: `${id++}`, type: 'point', x: elem.point.x, y: elem.point.y, fixed: true }]
      }

      idMap[elem.id] = primitives
      return primitives
    }).filter(Boolean)

    // Refresh references before collecting fixed solver primitives.
    // This is also needed by DimensionControls after feature-tree updates.
    this.constraints.forEach(constraint => constraint.update(tree))
    const parameters = this.creator && tree.findChild(this.creator.componentId)?.getParameters()
    this.constraints.filter(constraint => constraint instanceof Dimension && constraint.expression)
      .forEach(constraint => {
        constraint.distance = new Expression(
          constraint.expression,
          parameters,
          constraint.isAngular() ? 'angle' : 'length',
        ).getBase()
      })
    this.removeRedundantCoincidentConstraints()

    const fixedPoints = this.constraints
      .flatMap(constraint => constraint.items.map(item => item.curve()))
      .filter((item, index, items) => item instanceof SketchOrigin && items.indexOf(item) == index)

    fixedPoints.forEach(item => {
      const point = item.center()
      const primitive = { id: `${id++}`, type: 'point', x: point.x, y: point.y, fixed: true }
      idMap[item.id] = [primitive]
      primitives.push(primitive)
    })

    const pointPrimitive = item => {
      const curve = item.curve()
      const curvePrimitives = idMap[curve.id]
      if(curve instanceof Arc) return curvePrimitives[[2, 0, 1][item.index]]
      return curvePrimitives[item.index]
    }

    const pointOnCurveConstraint = (constraintId, pointRef, curve, curvePrim, temporary) => {
      const pointPrim = pointPrimitive(pointRef)
      if(!pointPrim) throw new Error('Unsupported point-on-curve constraint point')
      if(curve instanceof Spline) {
        const point = pointRef.curve().handles()[pointRef.index]
        const parameterName = `spline-parameter-${constraintId}`
        return [
          { type: 'free_param', name: parameterName, value: curve.unsample(point) },
          { id: constraintId, type: 'point_on_bspline', p_id: pointPrim.id, b_id: curvePrim.id, pointparam: parameterName, temporary },
        ]
      }
      const target = curve instanceof Line ? ['point_on_line_pl', 'l_id'] :
        curve instanceof Circle ? ['point_on_circle', 'c_id'] :
        ['point_on_arc', 'a_id']
      return { id: constraintId, type: target[0], p_id: pointPrim.id, [target[1]]: curvePrim.id, temporary }
    }

    // An arc has independently stored center, endpoint, angle, and radius
    // parameters in PlaneGCS. Arc rules are the internal equations that keep
    // those parameters describing one coherent piece of geometry.
    const arcRules = this.elements.concat(projections)
      .filter(elem => elem instanceof Arc)
      .map(elem => ({
        id: `${id++}`,
        type: 'arc_rules',
        a_id: idMap[elem.id].slice(-1)[0].id,
        temporary: true,
      }))

    // Projected point coordinates are fixed above, but PlaneGCS stores a
    // circle's radius as a separate free parameter. Lock that radius as
    // well; otherwise tangency can be "solved" by changing an invisible
    // working radius that is reset when the projection is regenerated.
    const projectionRadiusRules = projections
      .filter(elem => elem instanceof Circle)
      .map(elem => {
        const primitive = idMap[elem.id].slice(-1)[0]
        return {
          id: `${id++}`,
          type: 'circle_radius',
          c_id: primitive.id,
          radius: elem.radius,
        }
      })

    const constraints = [...arcRules, ...projectionRadiusRules].concat(this.constraints.flatMap(c => {
      if(c instanceof HorVertConstraint) {
        const pointPrims = c.items.length == 1 ?
          idMap[c.items[0].curve().id].slice(0, 2)
          :
          c.items.map(pointPrimitive)
        return { id: `${id++}`, type: c.isVertical ? 'vertical_pp' : 'horizontal_pp', p1_id: pointPrims[0].id, p2_id: pointPrims[1].id, temporary: c.temporary }

      } else if(c instanceof FixConstraint) {
        const item = c.items[0]
        const curve = c.items[0].curve()
        const pointPrims = item.index !== undefined ?
          [pointPrimitive(item)]
          :
          idMap[curve.id].slice(0, 2)
        return pointPrims.flatMap(point => [
          { id: `${id++}`, type: 'coordinate_x', p_id: point.id, x: point.x },
          { id: `${id++}`, type: 'coordinate_y', p_id: point.id, y: point.y },
        ])

      } else if(c instanceof TouchConstraint) {
        const pointRef = c.items.find(item => item.index !== undefined )
        const curveRef = c.items.find(item => item.index === undefined )
        const curve = curveRef.curve()

        const curvePrim = idMap[curve.id].slice(-1)[0]
        if(!supportsPointOnCurveConstraint(curve)) throw new Error('Unsupported Touch constraint geometry')
        return pointOnCurveConstraint(`${id++}`, pointRef, curve, curvePrim, c.temporary)

      } else if(c instanceof MidpointConstraint) {
        const pointRef = c.items.find(item => item.index !== undefined)
        const lineRef = c.items.find(item => item.index === undefined)
        const pointPrim = pointPrimitive(pointRef)
        const linePrim = idMap[lineRef.curve().id].slice(-1)[0]
        return [
          { id: `${id++}`, type: 'point_on_line_pl', p_id: pointPrim.id, l_id: linePrim.id, temporary: c.temporary },
          { id: `${id++}`, type: 'point_on_perp_bisector_pl', p_id: pointPrim.id, l_id: linePrim.id, temporary: c.temporary },
        ]

      } else if(c instanceof IntersectionConstraint) {
        const pointRef = c.items.find(item => item.index !== undefined)
        return c.items.filter(item => item.index === undefined).flatMap(item => {
          const curve = item.curve()
          const curvePrim = idMap[curve.id].slice(-1)[0]
          if(!supportsPointOnCurveConstraint(curve)) throw new Error('Unsupported Intersection constraint geometry')
          return pointOnCurveConstraint(`${id++}`, pointRef, curve, curvePrim, c.temporary)
        })

      } else if(c instanceof PerpendicularConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        return { id: `${id++}`, type: 'perpendicular_ll', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

      } else if(c instanceof ParallelConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        return { id: `${id++}`, type: 'parallel', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

      } else if(c instanceof EqualConstraint) {
        const items = c.items.map(item => ({
          curve: item.curve(),
          primitive: idMap[item.curve().id].slice(-1)[0],
        }))
        if(items.every(item => item.curve instanceof Line)) {
          return { id: `${id++}`, type: 'equal_length', l1_id: items[0].primitive.id, l2_id: items[1].primitive.id, temporary: c.temporary }
        }
        if(items.every(item => item.curve instanceof Circle)) {
          return { id: `${id++}`, type: 'equal_radius_cc', c1_id: items[0].primitive.id, c2_id: items[1].primitive.id, temporary: c.temporary }
        }
        if(items.every(item => item.curve instanceof Arc)) {
          return { id: `${id++}`, type: 'equal_radius_aa', a1_id: items[0].primitive.id, a2_id: items[1].primitive.id, temporary: c.temporary }
        }

        const circle = items.find(item => item.curve instanceof Circle)
        const arc = items.find(item => item.curve instanceof Arc)
        if(circle && arc) {
          return { id: `${id++}`, type: 'equal_radius_ca', c1_id: circle.primitive.id, a2_id: arc.primitive.id, temporary: c.temporary }
        }
        throw new Error('Unsupported Equal constraint geometry')

      } else if(c instanceof TangentConstraint) {
        const line = c.items.find(item => item.curve() instanceof Line)
        const circles = c.items.filter(item => item.curve() instanceof Circle)
        if(circles.length == 2) {
          const circlePrims = circles.map(item => idMap[item.curve().id].slice(-1)[0])
          return { id: `${id++}`, type: 'tangent_cc', c1_id: circlePrims[0].id, c2_id: circlePrims[1].id, temporary: c.temporary }
        }
        const spline = c.items.find(item => item.curve() instanceof Spline)
        const curve = spline || c.items.find(item => item.curve() instanceof Circle || item.curve() instanceof Arc)
        if(!curve) throw new Error('Unsupported Tangent constraint geometry')
        const linePrim = line && idMap[line.curve().id].slice(-1)[0]
        const curvePrim = idMap[curve.curve().id].slice(-1)[0]
        if(curve.curve() instanceof Spline) {
          const splineCurve = curve.curve()
          const other = line ? line.curve() : c.items.find(item => item.curve() instanceof Circle || item.curve() instanceof Arc).curve()
          const endpoint = splineCurve.endpoints()
            .map((point, index) => ({ index, distance: other.distanceTo(point) }))
            .minMaxBy(Math.min, candidate => candidate.distance)
          const tangentPrim = idMap[splineCurve.id].find(primitive =>
            primitive.type == 'line' && primitive.splineTangent == endpoint.index
          )
          if(line) {
            return { id: `${id++}`, type: 'parallel', l1_id: linePrim.id, l2_id: tangentPrim.id, temporary: c.temporary }
          }
          const target = c.items.find(item => item.curve() instanceof Circle || item.curve() instanceof Arc).curve()
          const targetPrim = idMap[target.id].slice(-1)[0]
          return target instanceof Circle ?
            { id: `${id++}`, type: 'tangent_lc', l_id: tangentPrim.id, c_id: targetPrim.id, temporary: c.temporary } :
            { id: `${id++}`, type: 'tangent_la', l_id: tangentPrim.id, a_id: targetPrim.id, temporary: c.temporary }
        }
        if(!line) throw new Error('Unsupported Tangent constraint geometry')
        return curve.curve() instanceof Circle ?
          { id: `${id++}`, type: 'tangent_lc', l_id: linePrim.id, c_id: curvePrim.id, temporary: c.temporary }
          :
          { id: `${id++}`, type: 'tangent_la', l_id: linePrim.id, a_id: curvePrim.id, temporary: c.temporary }

      } else if(c instanceof CoincidentConstraint) {
        const constraintPrims = c.items.map(pointPrimitive)
        return { id: `${id++}`, type: 'p2p_coincident', p1_id: constraintPrims[0].id, p2_id: constraintPrims[1].id, temporary: c.temporary }

      // Dimension
      } else if(c instanceof Dimension) {
        if(c.isCircleOffset()) {
          const [left, right] = c.items.map(item => idMap[item.curve().id].slice(-1)[0])
          const leftCenter = idMap[c.items[0].curve().id][0]
          const rightCenter = idMap[c.items[1].curve().id][0]
          const centersCoincident = this.coincidentPointsConnected(
            c.items[0].curve(), 0, c.items[1].curve(), 0
          )
          return [
            // Coincident centers provide the implicit concentric constraint.
            ...(centersCoincident ? [] : [{ id: `${id++}`, type: 'p2p_coincident', p1_id: leftCenter.id, p2_id: rightCenter.id, temporary: c.temporary }]),
            { id: `${id++}`, type: 'difference', param1: { o_id: left.id, prop: 'radius' }, param2: { o_id: right.id, prop: 'radius' }, difference: c.radiusDifferenceSign * c.distance, temporary: c.temporary },
          ]

        } else if(c.isPointDistance()) {
          // Circle centers participate in point distances even though the
          // circle itself is selected rather than an ElemRef.
          const [p1, p2] = c.items.map(item => item.curve() instanceof Circle ?
            idMap[item.curve().id][0] : pointPrimitive(item))
          return { id: `${id++}`, type: 'p2p_distance', p1_id: p1.id, p2_id: p2.id, distance: c.distance, temporary: c.temporary }

        } else if(c.items[0].curve() instanceof Circle) {
          // Circle diameter
          const circlePrim = idMap[c.items[0].curve().id].slice(-1)[0]
          return { id: `${id++}`, type: 'circle_diameter', c_id: circlePrim.id, diameter: c.distance, temporary: c.temporary }

        } else if(c.items[0].curve() instanceof Arc) {
          const arcPrim = idMap[c.items[0].curve().id].slice(-1)[0]
          return { id: `${id++}`, type: 'arc_radius', a_id: arcPrim.id, radius: c.distance, temporary: c.temporary }

        } else if(c.items.length == 1) {
          // Line length
          const [p1, p2] = idMap[c.items[0].curve().id]
          return { id: `${id++}`, type: 'p2p_distance', p1_id: p1.id, p2_id: p2.id, distance: c.distance, temporary: c.temporary }

        } else if(c.isAngular()) {
          // Line to line angle
          const [l1, l2] = c.items.map(item => idMap[item.curve().id].slice(-1)[0])
          return { id: `${id++}`, type: 'l2l_angle_ll', l1_id: l1.id, l2_id: l2.id, angle: c.solverAngle(), temporary: c.temporary }

        } else {
          // Line to line distance
          const pointPrim = idMap[c.items[0].curve().id][0]
          const linePrim = idMap[c.items[1].curve().id].slice(-1)[0]
          return { id: `${id++}`, type: 'p2l_distance', p_id: pointPrim.id, l_id: linePrim.id, distance: c.distance, temporary: c.temporary }
        }
      }
    }))

    // Treat the pointer position as a temporary driving target. Without
    // these constraints the dragged coordinates are only the solver's
    // initial guess, so it is free to move the endpoint while satisfying a
    // tangent (or any other) constraint, which makes the handle swim.
    if(draggedHandle) {
      const dragPrim = pointPrimitive({
        curve: () => draggedHandle.elem,
        index: draggedHandle.index,
      })
      const target = draggedHandle.elem.handles()[draggedHandle.index]
      if(dragPrim && target) constraints.push(
        { id: `${id++}`, type: 'coordinate_x', p_id: dragPrim.id, x: target.x, temporary: true },
        { id: `${id++}`, type: 'coordinate_y', p_id: dragPrim.id, y: target.y, temporary: true },
      )
    }

    // Solve
    const parameterGroups = draggedHandle ? [] : this.elements.map(elem =>
      idMap[elem.id].map(primitive => primitive.id)
    )
    const { results, conflicting, fullyConstrained } = window.oc.solveSystem(
      [...primitives, ...constraints],
      parameterGroups,
    )

    if(conflicting) {
      window.bus.emit('toast', 'Sketch was over-constrained')
      this.constraints.pop()
      this.solve(tree, draggedHandle)
      return
    }

    // Pointer-driving constraints temporarily remove freedom while dragging;
    // retain each element's last structural state until a normal solve.
    if(!draggedHandle) this.elements.forEach((elem, index) =>
      elem.fullyConstrained = fullyConstrained[index]
    )

    // Write back results
    const updatePrim = (prim) => results.find(res => res.id == prim.id )

    const vecFromPrim = (prim) => {
      const updated = updatePrim(prim)
      return new THREE.Vector3(updated.x, updated.y, 0.0)
    }

    this.elements.forEach(elem => {
      if(elem instanceof SketchPoint) {
        elem.setHandles([vecFromPrim(idMap[elem.id][0])])

      } else if(elem instanceof Line) {
        const [p1, p2] = idMap[elem.id]
        elem.setHandles([vecFromPrim(p1), vecFromPrim(p2)])

      } else if(elem instanceof Circle) {
        const [center, circle] = idMap[elem.id]
        const solvedCenter = vecFromPrim(center)
        const solvedRadius = Math.abs(updatePrim(circle).radius)
        if([solvedCenter.x, solvedCenter.y, solvedCenter.z, solvedRadius].every(Number.isFinite) && solvedRadius > EPSILON) {
          elem.radius = solvedRadius
          elem.setHandles([solvedCenter])
        }

      } else if(elem instanceof Arc) {
        const [start, end, center, arc] = idMap[elem.id]
        elem.setSolvedHandles(
          [vecFromPrim(center), vecFromPrim(start), vecFromPrim(end)],
          updatePrim(arc).radius,
        )

      } else if(elem instanceof Spline) {
        elem.setHandles(idMap[elem.id].filter(prim => prim.type == 'point').map(vecFromPrim))
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
    sketch.constraints.forEach(constraint => constraint.sketch = sketch )
    sketch.projections.forEach(projection => projection.sketch = sketch )
    context.sketches[sketch.id] = sketch
    return sketch
  }
}
Serialize.register(Sketch, 'Sketch')


export class ProjectedPoint {
  constructor(point, id) {
    this.point = point
    this.id = id
  }

  typename() { return 'Point Projection' }
  center() { return this.point }
  snapPoints() { return [this.point] }
  handles() { return [this.point] }
}


export class Projection {
  constructor(itemOrRef) {
    this.id = makeID()
    this.isReference = false
    this.itemRef = itemOrRef instanceof Edge ?
      new EdgeReference(itemOrRef)
      :
      (itemOrRef instanceof AxisHelper || itemOrRef instanceof PointHelper ?
        new HelperReference(itemOrRef)
        :
        itemOrRef
      )
  }

  update(tree) {
    this.itemRef.update(tree)
    const isElementPoint = this.itemRef instanceof ElemRef
    const item = isElementPoint ? this.itemRef.curve() : this.itemRef.getItem()
    const relative = this.relativeTransform(tree)

    if(isElementPoint || item instanceof PointHelper) {
      let point = isElementPoint ?
        item.handles()[this.itemRef.index].clone().applyMatrix4(item.sketch.workplane) :
        new THREE.Vector3().setFromMatrixPosition(item.getPoint())
      point.applyMatrix4(relative).applyMatrix4(this.sketch.workplane.clone().invert())
      point.z = 0
      const suffix = isElementPoint ? `/${this.itemRef.index}` : ''
      const elem = new ProjectedPoint(point, item.id + suffix + '/projected')
      return this.updateOutput(elem)
    }

    let curve
    if(item instanceof Edge) {
      // BRep_Tool.Curve()
      const shape = transformGeometry(item.geom(), relative).Shape()
      curve = new window.oc.oc.BRepAdaptor_Curve_2(window.oc.oc.TopoDS.Edge_1(shape))

    } else if(item instanceof AxisHelper) {
      const axis = relative.clone().multiply(item.transform)
      const geom = new window.oc.oc.GC_MakeSegment_1(
        ocPntFromVec(new THREE.Vector3().applyMatrix4(axis)),
        ocPntFromVec(new THREE.Vector3(0, 0, 20).applyMatrix4(axis)),
      ).Value().get()
      const handle = new window.oc.oc.Handle_Geom_Curve_2(geom)
      curve = new window.oc.oc.GeomAdaptor_Curve_2(handle)
    }

    const ax = ocAx3FromMatrix(this.sketch.workplane)
    const project = new window.oc.oc.ProjLib_ProjectOnPlane_2(ax)
    project.Load(curve.ShallowCopy(), EPSILON, true) // ShallowCopy upcasts BRepAdaptor_Curve -> Adaptor3d_Curve

    const projected = project.GetResult().get()
    const [u1, u2] = [projected.FirstParameter(), projected.LastParameter()]

    const plane = ocPlnFromMatrix(this.sketch.workplane)
    let curve2d = window.oc.oc.GeomAPI.To2d(projected.Curve(), plane)
    const handle = new window.oc.oc.Handle_Geom2d_Curve_2(curve2d.get())
    const trimmed = new window.oc.oc.Geom2d_TrimmedCurve(handle, u1, u2, true, true)

    const constructor = {
      [window.oc.oc.GeomAbs_CurveType.GeomAbs_Line.constructor]: Line,
      [window.oc.oc.GeomAbs_CurveType.GeomAbs_Circle.constructor]: Circle,
    }[project.GetType().constructor]

    if(constructor) {
      const elem = constructor.fromGeometry(trimmed, item.id + '/projected')
      return this.updateOutput(elem)

    } else {
      console.error(`Could not project ${project.getType().constructor} from ${curve.constructor}`)
    }

    return this.output
  }

  updateOutput(elem) {
    const existing = this.output
    if(existing && existing.constructor == elem.constructor) {
      if(existing instanceof SketchElement) existing.clear()
      Object.assign(existing, elem)
      elem = existing
    } else if(existing instanceof SketchElement) {
      existing.clear()
    }

    elem.sketch = this.sketch
    elem.projection = this
    if(elem instanceof SketchElement) elem.isReference = this.isReference
    this.output = elem
    return elem
  }

  relativeTransform(tree) {
    const sourceId = this.itemRef instanceof ElemRef ?
      this.itemRef.curveRef.componentId : this.itemRef.componentId
    const targetId = this.sketch.creator.componentOccurrenceId ||
      this.sketch.component?.id || this.sketch.creator.componentId
    const source = tree.findChild(sourceId)
    const target = tree.findChild(targetId)
    if(!source || !target) return new THREE.Matrix4()
    return relativeComponentTransform(source, target)
  }

  geometry() { return this.output }

  dump() {
    return {
      id: this.id,
      itemRef: this.itemRef,
      isReference: this.isReference,
    }
  }

  static undump(dump) {
    const projection = new Projection(dump.itemRef)
    projection.id = dump.id || projection.id
    projection.isReference = dump.isReference || false
    return projection
  }
}
Serialize.register(Projection, 'Projection')


export class ElemRef {
  constructor(curveOrRef, index) {
    const isReference = curveOrRef instanceof CurveReference ||
      curveOrRef instanceof SketchOriginReference
    this.curveRef = isReference ?
      curveOrRef
      :
      (curveOrRef instanceof SketchOrigin ? new SketchOriginReference(curveOrRef) : new CurveReference(curveOrRef))
    this.index = index === undefined && this.curve() instanceof SketchOrigin ? 0 : index
  }

  update(tree) {
    this.curveRef.update(tree)
  }

  curve() {
    return this.curveRef.getItem()
  }

  // isPoint() { return this.index !== undefined }

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

  dump() {
    return {
      items: this.items,
    }
  }

  static undump(dump) {
    const constraint = Object.create(this.prototype)
    Object.assign(constraint, dump)
    delete constraint.$class
    return constraint
  }
}

export class HorVertConstraint extends Constraint {
  static icon = 'ruler-horizontal'

  constructor(...items) {
    super(...items)
    const points = this.items.length == 1 ?
      this.items[0].curve().endpoints()
      :
      this.items.map(item => item.curve().handles()[item.index])
    const xDiff = Math.abs(points[0].x - points[1].x)
    const yDiff = Math.abs(points[0].y - points[1].y)
    this.isVertical = xDiff < yDiff
  }

  typename() { return (this.isVertical ? 'Vertical' : 'Horizontal') + ' Constraint' }

  icon() { return this.isVertical ? 'ruler-vertical' : 'ruler-horizontal' }

  dump() {
    return {
      ...super.dump(),
      isVertical: this.isVertical,
    }
  }
}
Serialize.register(HorVertConstraint, 'HorVertConstraint')

export class FixConstraint extends Constraint {
  static icon = 'lock'
  typename() { return 'Fix Constraint' }
}
Serialize.register(FixConstraint, 'FixConstraint')

export class CoincidentConstraint extends Constraint {
  static icon = 'dot-circle'
  typename() { return 'Coincident Constraint' }
}
Serialize.register(CoincidentConstraint, 'CoincidentConstraint')

export class TouchConstraint extends Constraint {
  static icon = 'asterisk'
  typename() { return 'Touch Constraint' }
}
Serialize.register(TouchConstraint, 'TouchConstraint')

export class MidpointConstraint extends Constraint {
  static icon = 'expand-alt'
  typename() { return 'Midpoint Constraint' }
}
Serialize.register(MidpointConstraint, 'MidpointConstraint')

export class IntersectionConstraint extends Constraint {
  static icon = 'crosshairs'
  typename() { return 'Intersection Constraint' }
}
Serialize.register(IntersectionConstraint, 'IntersectionConstraint')

export class PerpendicularConstraint extends Constraint {
  static icon = 'angle-up'
  typename() { return 'Perpendicular Constraint' }
}
Serialize.register(PerpendicularConstraint, 'PerpendicularConstraint')

export class ParallelConstraint extends Constraint {
  static icon = 'exchange-alt'
  typename() { return 'Parallel Constraint' }
}
Serialize.register(ParallelConstraint, 'ParallelConstraint')

export class EqualConstraint extends Constraint {
  static icon = 'equals'
  typename() { return 'Equal Constraint' }
}
Serialize.register(EqualConstraint, 'EqualConstraint')

export class TangentConstraint extends Constraint {
  static icon = 'bezier-curve'
  typename() { return 'Tangent Constraint' }
}
Serialize.register(TangentConstraint, 'TangentConstraint')

export class Dimension extends Constraint {
  static icon = 'ruler'
  typename() { return 'Dimension' }

  constructor(items, pos) {
    super(...items)
    this.position = pos
    this.expression = null
    if(this.isCircleOffset()) {
      const [left, right] = items
      this.distance = Math.abs(left.radius - right.radius)
      this.radiusDifferenceSign = Math.sign(left.radius - right.radius) || 1

    } else if(this.isPointDistance()) {
      const point = item => item.curve() instanceof Circle ? item.curve().center() : item.curve().handles()[item.index]
      const [a, b] = this.items.map(point)
      this.distance = a.distanceTo(b)

    } else if(items[0] instanceof Circle) {
      this.distance = items[0].radius * 2.0

    } else if(items[0] instanceof Arc) {
      this.distance = items[0].radius

    } else if(items.length == 1) {
      this.distance = items[0].length()

    } else if(this.isAngular()) {
      const rawAngle = signedLineAngle(items[0], items[1])
      const displayAngle = orientedLineAngle(items[0], items[1], pos)
      this.distance = Math.abs(THREE.MathUtils.radToDeg(displayAngle))
      this.angleSign = Math.sign(displayAngle) || 1
      this.angleOffset = normalizeDegrees(
        THREE.MathUtils.radToDeg(rawAngle) - this.angleSign * this.distance
      )

    } else {
      const [a, b] = items
      const aPoint = a.handles()[0].clone().sub(b.handles()[0])
      const dir = b.direction()
      this.distance = aPoint.clone().projectOnVector(dir).distanceTo(aPoint)
    }
  }

  isPointDistance() {
    return this.items.length == 2 && this.items.every(item => item.index !== undefined || item.curve() instanceof Circle)
  }

  isCircleOffset() {
    return this.items.length == 2 && this.items.every(item =>
      item.index === undefined && item.curve() instanceof Circle
    )
  }

  isAngular() {
    if(this.items.length != 2 || this.isPointDistance()) return false
    const curves = this.items.map(item => item.curve())
    if(!curves.every(curve => curve instanceof Line)) return false
    return curves[0].direction().normalize().cross(curves[1].direction().normalize()).lengthSq() >= 1e-8
  }

  solverAngle() {
    const degrees = (this.angleSign ?? 1) * this.distance + (this.angleOffset ?? 0)
    return THREE.MathUtils.degToRad(normalizeDegrees(degrees))
  }

  dump() {
    return {
      ...super.dump(),
      position: this.position,
      distance: this.distance,
      angleSign: this.angleSign,
      angleOffset: this.angleOffset,
      radiusDifferenceSign: this.radiusDifferenceSign,
      expression: this.expression,
    }
  }
}
Serialize.register(Dimension, 'Dimension')

function orientedLineAngle(left, right, position) {
  const curves = [left, right]
  const directions = curves.map(curve => curve.direction().normalize())
  const intersection = lineIntersection(curves[0], curves[1])
  if(intersection) {
    const towardPosition = position.clone().sub(intersection)
    directions.forEach(direction => {
      if(direction.dot(towardPosition) < 0) direction.negate()
    })
  }
  return signedDirectionsAngle(directions[0], directions[1])
}

function signedLineAngle(left, right) {
  return signedDirectionsAngle(left.direction().normalize(), right.direction().normalize())
}

function signedDirectionsAngle(left, right) {
  return Math.atan2(left.x * right.y - left.y * right.x, left.dot(right))
}

function normalizeDegrees(degrees) {
  return THREE.MathUtils.euclideanModulo(degrees + 180, 360) - 180
}

function lineIntersection(left, right) {
  const originL = left.handles()[0]
  const originR = right.handles()[0]
  const dirL = left.direction()
  const dirR = right.direction()
  const cross = dirL.x * dirR.y - dirL.y * dirR.x
  if(Math.abs(cross) < 1e-8) return
  const diff = originR.clone().sub(originL)
  const t = (diff.x * dirR.y - diff.y * dirR.x) / cross
  return originL.clone().add(dirL.multiplyScalar(t))
}
