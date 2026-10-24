import * as THREE from 'three'
import { nextTick, toRaw } from 'vue'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { Line, Circle, Arc, Spline, DerivedCurve, SketchElement, SketchPoint, supportsPointOnCurveConstraint } from './geom2d.js'
import { buildOffsetCurves, nearestOffsetFrame } from './sketch-offset.js'
import { Wire, Profile, Edge } from './geom3d.js'
import { AxisHelper, PointHelper } from './helpers.js'
import { EPSILON, cross2d } from './utils.js'
import { CurveReference, EdgeReference, HelperReference, SketchOriginReference } from './references.js'
import { relativeComponentTransform } from './assembly.js'
import Expression from './expression.js'
import { solveDrag, validGeometry } from './sketch-drag.js'
import { solveSystem } from './sketch-solver.js'
import { shapex as sx, transformFromMatrix, planeFromMatrix } from './shapex.js'
import { isModelingFailure } from './errors.js'

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
    this.history = null
    // this.componentId = componentId
  }

  typename() { return 'Sketch' }

  startHistory() {
    this.history = { states: [this.historySnapshot()], index: 0 }
  }

  historySnapshot(state = this.dump()) {
    return JSON.stringify(Serialize.serialize(state), (key, value) =>
      key == 'isReference' ? !!value : value)
  }

  recordHistory(state) {
    if(!this.history || this.restoringHistory) return
    const snapshot = this.historySnapshot(state)
    const { states, index } = this.history
    if(snapshot == states[index]) return
    states.splice(index + 1, states.length, snapshot)
    this.history.index++
  }

  get canUndo() { return !!this.history && this.history.index > 0 }
  get canRedo() { return !!this.history && this.history.index < this.history.states.length - 1 }

  restoreHistory(direction, tree) {
    if(direction < 0 ? !this.canUndo : !this.canRedo) return false
    this.history.index += direction
    const state = Serialize.parse(this.history.states[this.history.index])
    const previousElements = [...this.elements, ...this.projections
      .map(projection => projection.output).filter(elem => elem instanceof SketchElement)]
    Object.assign(this, state)
    this.elements.forEach(elem => elem.sketch = this)
    this.constraints.forEach(constraint => constraint.sketch = this)
    this.projections.forEach(projection => projection.sketch = this)
    this.projections.forEach(projection => projection.update(tree))
    this.constraints.forEach(constraint => constraint.update(tree))
    this.constraints.filter(constraint => constraint instanceof ProfileOffsetConstraint)
      .forEach(constraint => constraint.rebuild())
    this.solveHandle = null
    this.solveNeeded = false
    this.restoringHistory = true
    this.profileUpdateNeeded = true
    // Constraint proxies still reference the previous geometry until Vue updates.
    nextTick(() => previousElements.forEach(elem => elem.clear()))
    return true
  }

  add(elem) {
    if(!this.elements.includes(elem)) this.elements.push(elem)
    elem.sketch = this
    elem.fullyConstrained = false
  }

  addConstraint(constraint) {
    const existing = this.constraints.find(other => sameConstraint(other, constraint))
    if(existing) return existing
    if(constraint instanceof ConcentricConstraint) {
      const [left, right] = constraint.items.map(item => item.curve())
      if(this.coincidentPointsConnected(left, 0, right, 0)) return constraint
    }
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
    constraint.id ||= makeID()
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
    // Callers may pass freshly created raw objects, while the sketch's
    // reactive arrays hand out proxies. Compare raw identities.
    elem = toRaw(elem)
    const offsets = this.constraints.filter(constraint => constraint instanceof ProfileOffsetConstraint &&
      (toRaw(constraint) == elem || constraint.items.some(item => toRaw(item.curve()) == elem)))
    offsets.forEach(constraint => {
      constraint.outputs().forEach(curve => {
        curve = toRaw(curve)
        this.elements = this.elements.filter(element => toRaw(element) != curve)
        this.constraints = this.constraints.filter(other => other == constraint || !other.items.some(item => toRaw(item.curve()) == curve))
        curve.clear()
      })
      this.constraints = this.constraints.filter(other => other != constraint)
    })
    this.elements.forEach(element => element.fullyConstrained = false)
    if(elem instanceof SketchElement) {
      this.elements = this.elements.filter(e => toRaw(e) != elem )
      this.constraints = this.constraints.filter(c => !c.items.some(item => toRaw(item.curve()) == elem ) )
      if(elem.projection) this.projections = this.projections.filter(p => toRaw(p) != toRaw(elem.projection) )

    } else if(elem instanceof Constraint) {
      this.constraints = this.constraints.filter(c => toRaw(c) != elem )
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
        if(!(constraint instanceof CoincidentConstraint || constraint instanceof ConcentricConstraint)) return
        const items = constraint instanceof ConcentricConstraint ?
          constraint.items.map(item => new ElemRef(item.curveRef, 0)) : constraint.items
        if(!items.some(item =>
          item.curve() == point.elem && item.index == point.index
        )) return
        items.forEach(item => pending.push({ elem: item.curve(), index: item.index }))
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
    const endpointTouches = new Set()
    this.constraints.forEach(constraint => {
      const touchPoint = constraint instanceof TouchConstraint &&
        constraint.items.find(item => item.index !== undefined)
      const contact = touchPoint && touchPoint.curve().handles()[touchPoint.index].clone()
      constraint.items.forEach(item => {
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
        } else if(contact) {
          // A contact at a new trim boundary must bind the endpoint, rather
          // than allowing the point to slide along the underlying curve.
          const endpoint = replacements
            .filter(curve => !curve.isClosed() && curve.endpoints().some(point => point.almost(contact)))
            .map(curve => ({ curve, index: curve.handles().findIndex(point => point.almost(contact)) }))
            .find(candidate => candidate.index != -1)
          if(endpoint) {
            replacement = endpoint.curve
            replacementIndex = endpoint.index
            endpointTouches.add(constraint)
          } else {
            replacement = replacements.find(curve => curve.distanceTo(contact).almost(0.0))
            if(!replacement) {
              lostConstraints.add(constraint)
              return
            }
          }
        }

        item.curveRef.item = replacement
        item.curveRef.itemId = replacement.id
        item.index = replacementIndex
      })
    })

    this.constraints = this.constraints.filter(constraint => !lostConstraints.has(constraint))
      .map(constraint => endpointTouches.has(constraint) ?
        Object.assign(new CoincidentConstraint(...constraint.items), constraint) : constraint)
    this.removeRedundantCoincidentConstraints()
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

  solve(tree, draggedHandle, options={}) {
    if(draggedHandle && !options.transaction) {
      const snapshot = this.elements.filter(elem => !(elem instanceof DerivedCurve)).map(elem => ({
        elem, handles: elem.handles().map(point => point.clone()), radius: elem.radius,
      }))
      const restore = () => {
        snapshot.forEach(({ elem, handles, radius }) => {
          if(elem instanceof Arc) elem.setSolvedHandles(handles, radius)
          else {
            if(elem instanceof Circle) elem.radius = radius
            elem.setHandles(handles)
          }
        })
        this.constraints.filter(c => c instanceof ProfileOffsetConstraint)
          .forEach(c => c.rebuild({ preserveTopology: true }))
      }
      let accepted = false
      try {
        accepted = this.solve(tree, draggedHandle, { ...options, transaction: true }) !== false
        return accepted
      } finally {
        if(!accepted) restore()
      }
    }
    const { applyResults=true, offsetIteration=0 } = options
    let id = 1
    const idMap = {}
    const solverElements = this.elements.filter(elem => !(elem instanceof DerivedCurve))

    // Convert entire sketch to GCS format
    const projections = this.projections.map(p => p.update(tree) ).filter(elem => elem && !(elem instanceof DerivedCurve))
    const primitives = solverElements.concat(projections).flatMap(elem => {
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
          // PlaneGCS uses Cartesian counter-clockwise endpoint angles.
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

    const offsets = this.constraints.filter(constraint => constraint instanceof ProfileOffsetConstraint)
    const hasOffsetReferences = this.constraints.some(constraint => !(constraint instanceof ProfileOffsetConstraint) &&
      constraint.items.some(item => item.curve() instanceof DerivedCurve))
    if(applyResults && hasOffsetReferences && offsets.some(constraint =>
      !constraint.rebuild({ preserveTopology: !!draggedHandle }))) return false
    // Generated edges are read-only solver references. Their endpoints are
    // fixed points; curve contacts below use the exact curve's local tangent.
    this.elements.filter(elem => elem instanceof DerivedCurve).forEach(curve => {
      const points = curve.handles().map(point => ({
        id: `${id++}`, type: 'point', x: point.x, y: point.y, fixed: true,
      }))
      idMap[curve.id] = points
      primitives.push(...points)
    })
    const offsetContacts = []

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
      if(curve instanceof DerivedCurve) {
        const point = pointRef.curve().handles()[pointRef.index]
        const frame = curve.closestFrame(point)
        const a = { id: `${id++}`, type: 'point', x: frame.point.x, y: frame.point.y, fixed: true }
        const b = { id: `${id++}`, type: 'point', x: frame.point.x + frame.tangent.x, y: frame.point.y + frame.tangent.y, fixed: true }
        const line = { id: `${id++}`, type: 'line', p1_id: a.id, p2_id: b.id }
        primitives.push(a, b, line)
        offsetContacts.push({ pointRef, curve })
        return { id: constraintId, type: 'point_on_line_pl', p_id: pointPrim.id, l_id: line.id, temporary }
      }
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

    const tangentToRoundCurve = (curve, endpointIndices, tangentPrim, target, temporary) => {
      const targetPrim = idMap[target.id].at(-1)
      const index = endpointIndices.find(index => {
        const connected = item => this.coincidentPointsConnected(curve, index, item.curve(), item.index)
        return (target instanceof Arc && [1, 2].some(targetIndex =>
          this.coincidentPointsConnected(curve, index, target, targetIndex)
        )) || this.constraints.some(constraint =>
          (constraint instanceof TouchConstraint || constraint instanceof IntersectionConstraint) &&
          constraint.items.some(item => item.index === undefined && item.curve() == target) &&
          constraint.items.some(item => item.index !== undefined && connected(item))
        )
      })
      if(index !== undefined) {
        // At an attached endpoint, point-on-circle and line-distance
        // tangency have dependent gradients. Radius perpendicularity is
        // independent of the contact equation and remains stable on drag.
        const radial = {
          id: `${id++}`, type: 'line', p1_id: targetPrim.c_id,
          p2_id: pointPrimitive({ curve: () => curve, index }).id,
        }
        primitives.push(radial)
        return { id: `${id++}`, type: 'perpendicular_ll', l1_id: tangentPrim.id, l2_id: radial.id, temporary }
      }
      return target instanceof Circle ?
        { id: `${id++}`, type: 'tangent_lc', l_id: tangentPrim.id, c_id: targetPrim.id, temporary } :
        { id: `${id++}`, type: 'tangent_la', l_id: tangentPrim.id, a_id: targetPrim.id, temporary }
    }

    // Arcs joined at an endpoint are tangent where both centers lie on the
    // contact's radius; like the radial rule above, this stays independent
    // of the coincidence.
    const tangentRoundCurves = (first, second, temporary) => {
      const [firstPrim, secondPrim] = [first, second].map(curve => idMap[curve.id].at(-1))
      const contact = first instanceof Arc && second instanceof Arc && [1, 2].flatMap(i => [1, 2].map(j => [i, j]))
        .find(([i, j]) => this.coincidentPointsConnected(first, i, second, j))
      if(contact) {
        const radial = {
          id: `${id++}`, type: 'line', p1_id: firstPrim.c_id,
          p2_id: pointPrimitive({ curve: () => first, index: contact[0] }).id,
        }
        primitives.push(radial)
        return { id: `${id++}`, type: 'point_on_line_pl', p_id: secondPrim.c_id, l_id: radial.id, temporary }
      }
      if(first instanceof Arc && second instanceof Arc) {
        return { id: `${id++}`, type: 'tangent_aa', a1_id: firstPrim.id, a2_id: secondPrim.id, temporary }
      }
      const [circle, arc] = first instanceof Circle ? [firstPrim, secondPrim] : [secondPrim, firstPrim]
      return { id: `${id++}`, type: 'tangent_ca', c_id: circle.id, a_id: arc.id, temporary }
    }

    // An arc has independently stored center, endpoint, angle, and radius
    // parameters in PlaneGCS. Arc rules are the internal equations that keep
    // those parameters describing one coherent piece of geometry.
    const arcRules = solverElements.concat(projections)
      .filter(elem => elem instanceof Arc)
      .map(elem => ({
        id: `${id++}`,
        type: 'arc_rules',
        a_id: idMap[elem.id].slice(-1)[0].id,
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
      if(c.items.some(item => item.curve() instanceof DerivedCurve && !item.curve().handles().length)) return []
      if(c instanceof ProfileOffsetConstraint) {
        return [] // Exact offsets are regenerated after their sources solve.
      } else if(c instanceof HorVertConstraint) {
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
          idMap[curve.id].filter(primitive => primitive.type == 'point')
        const fixed = pointPrims.flatMap(point => [
          { id: `${id++}`, type: 'coordinate_x', p_id: point.id, x: point.x },
          { id: `${id++}`, type: 'coordinate_y', p_id: point.id, y: point.y },
        ])
        if(item.index === undefined && curve instanceof Circle) fixed.push({
          id: `${id++}`, type: 'circle_radius', c_id: idMap[curve.id].at(-1).id, radius: curve.radius,
        })
        return fixed

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
        const linePrimitive = item => {
          const curve = item.curve()
          return curve instanceof Spline ? idMap[curve.id].find(primitive =>
            primitive.type == 'line' && primitive.splineTangent == (item.index == 0 ? 0 : 1)
          ) : idMap[curve.id].at(-1)
        }
        const round = c.items.find(item => item.curve() instanceof Circle || item.curve() instanceof Arc)
        if(round) {
          const line = c.items.find(item => item !== round)
          const roundPrim = idMap[round.curve().id].at(-1)
          const linePrim = linePrimitive(line)
          return { id: `${id++}`, type: 'point_on_line_pl', p_id: roundPrim.c_id, l_id: linePrim.id, temporary: c.temporary }
        }
        const constraintPrims = c.items.map(linePrimitive)
        return { id: `${id++}`, type: 'perpendicular_ll', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

      } else if(c instanceof ParallelConstraint) {
        const constraintPrims = c.items.map(item => idMap[item.curve().id].slice(-1)[0] )
        return { id: `${id++}`, type: 'parallel', l1_id: constraintPrims[0].id, l2_id: constraintPrims[1].id, temporary: c.temporary }

      } else if(c instanceof ConcentricConstraint) {
        const centers = c.items.map(item => idMap[item.curve().id].at(-1).c_id)
        return { id: `${id++}`, type: 'p2p_coincident', p1_id: centers[0], p2_id: centers[1], temporary: c.temporary }

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
        if(curve.curve() instanceof Spline) {
          const splineCurve = curve.curve()
          const other = line ? line.curve() : c.items.find(item => item.curve() instanceof Circle || item.curve() instanceof Arc).curve()
          const endpoint = spline.index !== undefined ?
            { index: spline.index == 0 ? 0 : 1 } : splineCurve.endpoints()
            .map((point, index) => ({ index, distance: other.distanceTo(point) }))
            .minMaxBy(Math.min, candidate => candidate.distance)
          const tangentPrim = idMap[splineCurve.id].find(primitive =>
            primitive.type == 'line' && primitive.splineTangent == endpoint.index
          )
          if(line) {
            return { id: `${id++}`, type: 'parallel', l1_id: linePrim.id, l2_id: tangentPrim.id, temporary: c.temporary }
          }
          const target = c.items.find(item => item.curve() instanceof Circle || item.curve() instanceof Arc).curve()
          return tangentToRoundCurve(splineCurve,
            [endpoint.index == 0 ? 0 : splineCurve.points.length - 1], tangentPrim, target, c.temporary)
        }
        if(!line) return tangentRoundCurves(...c.items.map(item => item.curve()), c.temporary)
        return tangentToRoundCurve(line.curve(), [0, 1], linePrim, curve.curve(), c.temporary)

      } else if(c instanceof CoincidentConstraint) {
        const constraintPrims = c.items.map(pointPrimitive)
        return { id: `${id++}`, type: 'p2p_coincident', p1_id: constraintPrims[0].id, p2_id: constraintPrims[1].id, temporary: c.temporary }

      // Dimension
      } else if(c instanceof OffsetConstraint) {
        const [source, target] = c.items.map(item => idMap[item.curve().id])
        const left = source.at(-1)
        const right = target.at(-1)
        if(c.items[0].curve() instanceof Line) {
          const connector = { id: `${id++}`, type: 'line', p1_id: source[0].id, p2_id: target[0].id }
          primitives.push(connector)
          return [
            { id: `${id++}`, type: 'parallel', l1_id: left.id, l2_id: right.id },
            { id: `${id++}`, type: 'equal_length', l1_id: left.id, l2_id: right.id },
            { id: `${id++}`, type: 'perpendicular_ll', l1_id: left.id, l2_id: connector.id },
            { id: `${id++}`, type: 'p2p_distance', p1_id: source[0].id, p2_id: target[0].id, distance: c.distance },
          ]
        }
        return [
          { id: `${id++}`, type: 'p2p_coincident', p1_id: left.c_id, p2_id: right.c_id },
          { id: `${id++}`, type: 'difference', param1: { o_id: right.id, prop: 'radius' }, param2: { o_id: left.id, prop: 'radius' }, difference: c.radiusDifferenceSign * c.distance },
          ...(c.items[0].curve() instanceof Arc ? ['start_angle', 'end_angle'].map(prop => ({
            id: `${id++}`, type: 'difference', param1: { o_id: left.id, prop }, param2: { o_id: right.id, prop }, difference: 0,
          })) : []),
        ]
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

    // Pointer targets are separate from the accepted geometry. The drag
    // solver introduces them as temporary objectives in small trial steps.
    const dragTargets = []
    if(draggedHandle) {
      const targets = draggedHandle.targets || [{
        elem: draggedHandle.elem, index: draggedHandle.index,
        point: draggedHandle.elem.handles()[draggedHandle.index],
      }]
      targets.forEach(target => {
        if(target.radius !== undefined) {
          const primitive = idMap[target.elem.id].at(-1)
          dragTargets.push({ id: primitive.id, type: primitive.type, property: 'radius', value: target.radius })
        } else {
          const primitive = pointPrimitive({ curve: () => target.elem, index: target.index })
          for(const property of ['x', 'y']) dragTargets.push({
            id: primitive.id, property, value: target.point[property],
          })
        }
      })
    }

    // Solve
    const parameterGroups = draggedHandle ? [] : solverElements.map(elem =>
      idMap[elem.id].map(primitive => primitive.id)
    )
    const solution = draggedHandle ?
      solveDrag(primitives, constraints, dragTargets, solveSystem) :
      solveSystem([...primitives, ...constraints], parameterGroups)
    if(solution.conflicting || solution.failed) {
      if(!draggedHandle && applyResults) window.bus.emit('toast', solution.conflicting ? 'Sketch was over-constrained' : 'Could not solve sketch')
      return false
    }
    const { results, fullyConstrained } = solution

    // Pointer-driving constraints temporarily remove freedom while dragging;
    // retain each element's last structural state until a normal solve.
    if(!draggedHandle) solverElements.forEach((elem, index) =>
      elem.fullyConstrained = fullyConstrained[index]
    )

    // A status refresh after pointer release must not apply the unconstrained
    // solver result: doing so can move the geometry away from the position
    // established by the temporary pointer-driving constraints.
    if(!applyResults) return
    if(!validGeometry(results)) return false

    // Write back results
    const updatePrim = (prim) => results.find(res => res.id == prim.id )

    const vecFromPrim = (prim) => {
      const updated = updatePrim(prim)
      return new THREE.Vector3(updated.x, updated.y, 0.0)
    }

    for(const elem of solverElements) {
      const primitives = idMap[elem.id]
      if(elem instanceof Arc) {
        const [start, end, center, arc] = primitives
        if(!elem.setSolvedHandles(
          [vecFromPrim(center), vecFromPrim(start), vecFromPrim(end)],
          updatePrim(arc).radius,
        )) return false
      } else {
        if(elem instanceof Circle) elem.radius = updatePrim(primitives.at(-1)).radius
        elem.setHandles(primitives.filter(prim => prim.type == 'point').map(vecFromPrim))
      }
    }
    if(draggedHandle && this.constraints.some(constraint => {
      if(!(constraint instanceof TouchConstraint || constraint instanceof IntersectionConstraint)) return false
      const pointRef = constraint.items.find(item => item.index !== undefined)
      const point = pointRef.curve().handles()[pointRef.index]
      return constraint.items.some(item => item.index === undefined && item.curve() instanceof Spline &&
        !(item.curve().distanceTo(point) <= EPSILON / 10))
    })) return false

    if(offsets.some(constraint => !constraint.rebuild({ preserveTopology: !!draggedHandle }))) return false

    // PlaneGCS has no exact offset-curve primitive. Re-linearize contacts
    // against the regenerated curve until they meet the actual geometry,
    // retaining the contact's sliding degree of freedom along that curve.
    const toleranceSquared = EPSILON * EPSILON / 100
    const contactMoved = offsetContacts.some(({ pointRef, curve }) => curve.geom && !curve.invalid &&
      curve.closestFrame(pointRef.curve().handles()[pointRef.index]).point
        .distanceToSquared(pointRef.curve().handles()[pointRef.index]) > toleranceSquared)
    const endpointMoved = this.constraints.some(constraint => !(constraint instanceof ProfileOffsetConstraint) &&
      constraint.items.some(item => {
        const curve = item.curve()
        if(!(curve instanceof DerivedCurve) || item.index === undefined || !curve.handles().length) return false
        const previous = idMap[curve.id]?.[item.index]
        const point = curve.handles()[item.index]
        return previous && (point.x - previous.x) ** 2 + (point.y - previous.y) ** 2 > toleranceSquared
      }))
    if(contactMoved || endpointMoved) {
      if(offsetIteration < 15) return this.solve(tree, draggedHandle, { ...options, offsetIteration: offsetIteration + 1 })
      if(!draggedHandle) window.bus?.emit('toast', 'Could not solve the attachment to the offset curve')
      return false
    }
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
      new EdgeReference(itemOrRef) :
      itemOrRef instanceof AxisHelper || itemOrRef instanceof PointHelper ?
        new HelperReference(itemOrRef) :
        itemOrRef
  }

  update(tree) {
    this.error = null
    const error = this.itemRef.update(tree)
    if(error) return this.invalidate(error.msg)
    try {
      return this.projectGeometry(tree)
    } catch(error) {
      if(!isModelingFailure(error)) throw error
      return this.invalidate('This geometry cannot be projected onto the sketch plane')
    }
  }

  invalidate(message) {
    if(this.output instanceof SketchElement) this.output.clear()
    this.output = undefined
    this.error = message
  }

  projectGeometry(tree) {
    const isElementPoint = this.itemRef instanceof ElemRef
    const item = isElementPoint ? this.itemRef.curve() : this.itemRef.getItem()
    const relative = this.relativeTransform(tree)

    if(isElementPoint || item instanceof PointHelper) {
      const point = isElementPoint ? item.handles()[this.itemRef.index].clone().applyMatrix4(item.sketch.workplane)
        : new THREE.Vector3().setFromMatrixPosition(item.getPoint())
      point.applyMatrix4(relative).applyMatrix4(this.sketch.workplane.clone().invert())
      point.z = 0
      return this.updateOutput(new ProjectedPoint(point, this.id + (isElementPoint ? '/' + this.itemRef.index : '') + '/projected'))
    }

    const geometry = item instanceof Edge ? item.geom().trimmedCurve() : item.geom()
    const transformed = geometry.transformed(transformFromMatrix(relative))
    const result = transformed.projectedOnto(planeFromMatrix(this.sketch.workplane))
    const id = this.id + '/projected'
    if(result.kind == 'point') return this.updateOutput(new ProjectedPoint(new THREE.Vector3(result.point.x, result.point.y, 0), id))
    const curve = result.curve, basis = curve.curve()
    const Type = basis instanceof sx.Line2 ? Line : basis instanceof sx.Circle2 ?
      Math.abs(Math.abs(curve.range().span()) - Math.PI * 2) <= EPSILON ? Circle : Arc :
      DerivedCurve
    return this.updateOutput(Type.fromGeometry(curve, id))
  }

  updateOutput(elem) {
    const existing = this.output
    if(existing instanceof SketchElement) existing.clear()
    if(existing && existing.constructor == elem.constructor) {
      Object.assign(existing, elem)
      elem = existing
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

  typename() { return this.index === undefined ? this.curve().typename() : 'Point' }

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
      id: this.id,
      items: this.items,
    }
  }

  static undump(dump) {
    const constraint = Object.create(this.prototype)
    Object.assign(constraint, dump)
    constraint.id ||= makeID()
    delete constraint.$class
    return constraint
  }
}

export class HorVertConstraint extends Constraint {
  static icon = 'constraint-horizontal'

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

  icon() { return this.isVertical ? 'constraint-vertical' : 'constraint-horizontal' }

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
  static icon = 'constraint-coincident'
  typename() { return 'Coincident Constraint' }
}
Serialize.register(CoincidentConstraint, 'CoincidentConstraint')

export class TouchConstraint extends Constraint {
  static icon = 'constraint-touch'
  typename() { return 'Touch Constraint' }
}
Serialize.register(TouchConstraint, 'TouchConstraint')

export class MidpointConstraint extends Constraint {
  static icon = 'constraint-midpoint'
  typename() { return 'Midpoint Constraint' }
}
Serialize.register(MidpointConstraint, 'MidpointConstraint')

export class IntersectionConstraint extends Constraint {
  static icon = 'constraint-intersection'
  typename() { return 'Intersection Constraint' }
}
Serialize.register(IntersectionConstraint, 'IntersectionConstraint')

export class PerpendicularConstraint extends Constraint {
  static icon = 'constraint-perpendicular'
  typename() { return 'Perpendicular Constraint' }
}
Serialize.register(PerpendicularConstraint, 'PerpendicularConstraint')

export class ParallelConstraint extends Constraint {
  static icon = 'constraint-parallel'
  typename() { return 'Parallel Constraint' }
}
Serialize.register(ParallelConstraint, 'ParallelConstraint')

export class EqualConstraint extends Constraint {
  static icon = 'equals'
  typename() { return 'Equal Constraint' }
}
Serialize.register(EqualConstraint, 'EqualConstraint')

export class ConcentricConstraint extends Constraint {
  static icon = 'constraint-concentric'
  typename() { return 'Concentric Constraint' }
}
Serialize.register(ConcentricConstraint, 'ConcentricConstraint')

export class TangentConstraint extends Constraint {
  static icon = 'constraint-tangent'
  typename() { return 'Tangent Constraint' }
}
Serialize.register(TangentConstraint, 'TangentConstraint')

export class Dimension extends Constraint {
  static icon = 'dimension'
  typename() { return 'Dimension' }

  constructor(items, pos, distance) {
    super(...items)
    this.position = pos
    this.expression = null
    if(distance !== undefined) {
      this.distance = distance
      return
    }
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

export class OffsetConstraint extends Dimension {
  static icon = 'sketch-offset'
  typename() { return 'Offset' }

  constructor(items, position) {
    super(items, position)
    if(this.isCircleOffset()) {
      this.distance = Math.abs(items[0].radius - items[1].radius)
      this.radiusDifferenceSign = Math.sign(items[0].radius - items[1].radius) || 1
    }
  }

  isCircleOffset() {
    return this.items.every(item => item.curve() instanceof Circle || item.curve() instanceof Arc)
  }

  isAngular() { return false }
}
Serialize.register(OffsetConstraint, 'OffsetConstraint')

export class ProfileOffsetConstraint extends Dimension {
  static icon = 'sketch-offset'
  typename() { return 'Offset' }

  constructor(groups, outputs, position, distance) {
    const sources = [...new Set(groups.flat().map(entry => entry.curve))]
    super([...sources, ...outputs], position, Math.abs(distance))
    this.sourceCount = sources.length
    this.side = Math.sign(distance)
    this.groups = groups.map(entries => entries.map(entry => ({
      source: sources.indexOf(entry.curve), start: entry.start, end: entry.end,
    })))
    this.closed = groups.map(entries => entries[0].curve.sample(entries[0].start).almost(entries.at(-1).curve.sample(entries.at(-1).end)))
    this.validDistance = this.distance
    this.validExpression = this.expression
  }

  isCircleOffset() { return false }
  isPointDistance() { return false }
  isAngular() { return false }
  outputs() { return this.items.slice(this.sourceCount).map(item => item.curve()).filter(Boolean) }

  sourceGroups() {
    return this.groups.map((entries, index) => Object.assign(entries.map(entry => ({
      ...entry, curve: this.items[entry.source].curve(),
    })), { closed: this.closed?.[index] }))
  }

  dimensionPoints() {
    const frame = nearestOffsetFrame(this.sourceGroups(), this.position)
    return [frame.point, frame.point.clone().addScaledVector(frame.normal, this.side * this.distance)]
  }

  rebuild({ preserveTopology=false }={}) {
    let curves
    let accepted = true
    try {
      curves = buildOffsetCurves(this.sourceGroups(), this.side * this.distance)
    } catch(error) {
      if(!isModelingFailure(error)) throw error
      if(preserveTopology) return false
      accepted = false
      if(this.validDistance !== undefined) {
        this.distance = this.validDistance
        this.expression = this.validExpression
      }
      window.bus?.emit('toast', error.message)
      try {
        // A rejected value keeps the last valid offset, regenerated against
        // the current source. Never leave stale geometry after a source edit.
        curves = buildOffsetCurves(this.sourceGroups(), this.side * this.distance)
      } catch(error) {
        if(!isModelingFailure(error)) throw error
        this.outputs().forEach(curve => curve.invalid = true)
        return false
      }
    }
    const previous = this.outputs()
    if(preserveTopology && curves.length != previous.length) {
      curves.forEach(curve => curve.clear())
      return false
    }
    const outputs = curves.map((curve, index) => {
      const existing = previous[index]
      if(existing) {
        existing.update(curve.geom())
        existing.invalid = false
        existing.fullyConstrained = this.items.slice(0, this.sourceCount).every(item => item.curve().fullyConstrained)
        curve.geom = null // Transfer ownership without deleting the new curve.
        return existing
      }
      curve.isReference = this.items[0].curve().isReference
      this.sketch.add(curve)
      return curve
    })
    previous.slice(curves.length).forEach(curve => {
      this.sketch.elements = this.sketch.elements.filter(element => element != curve)
      this.sketch.constraints = this.sketch.constraints.filter(constraint => constraint == this ||
        !constraint.items.some(item => item.curve() == curve))
      curve.clear()
    })
    this.items = [...this.items.slice(0, this.sourceCount), ...outputs.map(curve => new ElemRef(curve))]
    this.validDistance = this.distance
    this.validExpression = this.expression
    return accepted
  }

  dump() {
    return { ...super.dump(), sourceCount: this.sourceCount, side: this.side, groups: this.groups, closed: this.closed }
  }
}
Serialize.register(ProfileOffsetConstraint, 'ProfileOffsetConstraint')

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
