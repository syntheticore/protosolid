import * as THREE from 'three'
import { Line, Circle, Arc, Spline, OffsetCurve, supportsPointOnCurveConstraint } from './core/geom2d.js'
import { cross2d, vecFromOc } from './core/utils.js'
import { CoincidentConstraint, FixConstraint, HorVertConstraint, ProfileOffsetConstraint } from './core/sketch.js'

const snapDistance = 14 // px
const maxSnapReferences = 5
const lineSnapAngle = THREE.MathUtils.degToRad(7.5)
const fullTurn = Math.PI * 2.0

function positiveAngle(angle) {
  return ((angle % fullTurn) + fullTurn) % fullTurn
}

function projectToGuide(point, guide) {
  return guide.start.clone().addScaledVector(guide.direction, point.clone().sub(guide.start).dot(guide.direction))
}

function snapTargetLines(target) {
  return [target?.elem, target?.pointTarget?.elem, ...(target?.curves || [])]
    .filter(elem => elem instanceof Line)
}

function closestPointOnCurve(elem, point) {
  if(elem instanceof OffsetCurve) return elem.closestFrame(point).point
  if(elem instanceof Spline) {
    const parameter = elem.unsample(point)
    if(parameter === undefined) return
    return vecFromOc(elem.geom().get().Value(parameter))
  }

  if(elem instanceof Line) {
    const [start, end] = elem.endpoints()
    const segment = end.clone().sub(start)
    const lengthSquared = segment.lengthSq()
    if(lengthSquared.almost(0.0)) return
    const parameter = THREE.MathUtils.clamp(point.clone().sub(start).dot(segment) / lengthSquared, 0.0, 1.0)
    return start.clone().addScaledVector(segment, parameter)
  }

  const center = elem.center()
  const radial = point.clone().sub(center)
  if(elem instanceof Circle) {
    if(radial.lengthSq().almost(0.0)) radial.set(1.0, 0.0, 0.0)
    return center.add(radial.normalize().multiplyScalar(elem.radius))
  }

  if(elem instanceof Arc) {
    const endpoints = elem.endpoints()
    if(radial.lengthSq().almost(0.0)) {
      return endpoints.minMaxBy(Math.min, endpoint => endpoint.distanceTo(point)).clone()
    }

    const projected = center.clone().add(radial.normalize().multiplyScalar(elem.radius))
    const angles = endpoints.map(endpoint =>
      Math.atan2(endpoint.y - center.y, endpoint.x - center.x)
    )
    const candidateAngle = Math.atan2(projected.y - center.y, projected.x - center.x)
    const direction = elem.forward ? -1.0 : 1.0
    const candidateSweep = positiveAngle(direction * (candidateAngle - angles[0]))
    const arcSweep = positiveAngle(direction * (angles[1] - angles[0]))
    if(candidateSweep <= arcSweep) return projected
    return endpoints.minMaxBy(Math.min, endpoint => endpoint.distanceTo(point)).clone()
  }
}

function guideIntersection(left, right) {
  const denominator = cross2d(left.direction, right.direction)
  if(denominator.almost(0.0)) return
  const parameter = cross2d(right.start.clone().sub(left.start), right.direction) / denominator
  return left.start.clone().addScaledVector(left.direction, parameter)
}

function curveGuideIntersections(elem, guide) {
  // Curve snapping is exact for splines, but guide/spline intersections
  // require a separate unbounded-line projection. Leave guide snapping out
  // rather than falling through to the circle-specific center/radius path.
  if(elem instanceof Spline || elem instanceof OffsetCurve) return []

  if(elem instanceof Line) {
    const [start, end] = elem.endpoints()
    const delta = end.clone().sub(start)
    const denominator = cross2d(delta, guide.direction)
    if(denominator.almost(0.0)) return []
    const parameter = cross2d(guide.start.clone().sub(start), guide.direction) / denominator
    if(parameter < 0.0 || parameter > 1.0) return []
    return [start.clone().addScaledVector(delta, parameter)]
  }

  const center = elem.center()
  const foot = projectToGuide(center, guide)
  const remainder = elem.radius * elem.radius - center.distanceToSquared(foot)
  if(remainder < 0.0) return []
  const span = Math.sqrt(Math.max(0.0, remainder))
  const points = [-span, span].map(other => foot.clone().addScaledVector(guide.direction, other))
  const unique = points.filter((point, index) => !index || !point.almost(points[0]))
  if(elem instanceof Circle) return unique
  return unique.filter(point => closestPointOnCurve(elem, point).almost(point))
}

export default class Snapper {
  constructor(viewport, updateView) {
    this.viewport = viewport
    this.updateView = updateView
    this.guides = []
    this.lastSnaps = []
    this.lastLines = []
    this.snapAnchor = null
    this.planeTransform = new THREE.Matrix4()
  }

  reset() {
    this.guides = []
    this.snapAnchor = null
    this.lastSnaps = []
    this.lastLines = []
    this.snapped = {}
    this.updateView([], null)
  }

  snap(vec, coords, snapToGuides, snapToPoints, localSpace) {
    this.guides = []
    this.snapped = {}
    this.snapAnchor = null
    if(localSpace) {
      vec = this.snapToGuides(vec, snapToGuides, snapToPoints)
      if(vec) vec.z = 0.0 //XXX project vec to plane before snapping
    }
    this.updateView(this.guides, snapToPoints ? this.snapAnchor : null)
    return vec
  }

  getActiveHandleConnections() {
    const connected = new Set()
    const visitedConstraints = new Set()
    this.getActiveCoincidentPoints().forEach(point => {
      connected.add(point.elem)

      this.viewport.document.activeSketch.constraints.forEach(constraint => {
        // A source cannot attach to its own generated offset: that would
        // create a circular dependency while dragging its control points.
        if(constraint instanceof ProfileOffsetConstraint && constraint.items.slice(0, constraint.sourceCount)
          .some(item => item.curve() == point.elem)) {
          constraint.outputs().forEach(curve => connected.add(curve))
        }
        if(visitedConstraints.has(constraint) || !constraint.items.some(item =>
          item.curve() == point.elem && item.index == point.index
        )) return
        visitedConstraints.add(constraint)
        constraint.items.forEach(item => connected.add(item.curve()))
      })
    })
    return connected
  }

  getActiveCoincidentPoints() {
    const handle = this.viewport.activeHandle
    if(!handle) return []
    const sketch = this.viewport.document.activeSketch
    const pending = [{ elem: handle.elem, index: handle.index }]
    const points = []
    while(pending.length) {
      const point = pending.pop()
      if(points.some(other => other.elem == point.elem && other.index == point.index)) continue
      points.push(point)
      sketch.constraints.forEach(constraint => {
        if(!(constraint instanceof CoincidentConstraint) || !constraint.items.some(item =>
          item.curve() == point.elem && item.index == point.index
        )) return
        constraint.items.forEach(item => pending.push({ elem: item.curve(), index: item.index }))
      })
    }
    return points
  }

  activeHandleReachability() {
    // This is a targeted pre-filter for fixed points and fixed-axis HorVert
    // constraints, including those reached through coincident point groups.
    // It is not a general feasibility test for every constraint combination;
    // that would require a non-mutating temporary solver pass.
    const sketch = this.viewport.document.activeSketch
    const points = this.getActiveCoincidentPoints()
    if(!points.length) return () => true
    const isActivePoint = item => points.some(point =>
      item.curve() == point.elem && item.index == point.index
    )
    const isFixedPoint = item => {
      const curve = item.curve()
      if(curve == sketch.origin() || curve.projection || curve instanceof OffsetCurve) return true
      return sketch.constraints.some(constraint => constraint instanceof FixConstraint &&
        constraint.items.some(fixed => fixed.curve() == curve &&
          (fixed.index === undefined || fixed.index == item.index)
        )
      )
    }
    const pointPosition = item => item.curve().handles()[item.index]

    return candidate => sketch.constraints.every(constraint => {
      if(constraint instanceof FixConstraint) {
        const fixedPoint = points.find(point => constraint.items.some(item =>
          item.curve() == point.elem && (item.index === undefined || item.index == point.index)
        ))
        if(fixedPoint) return candidate.almost(fixedPoint.elem.handles()[fixedPoint.index])
      }
      if(!(constraint instanceof HorVertConstraint) || constraint.items.length != 2) return true
      const activeItem = constraint.items.find(isActivePoint)
      const otherItem = activeItem && constraint.items.find(item => item != activeItem)
      if(!otherItem || isActivePoint(otherItem) || !isFixedPoint(otherItem)) return true
      const otherPoint = pointPosition(otherItem)
      return constraint.isVertical ? candidate.x.almost(otherPoint.x) : candidate.y.almost(otherPoint.y)
    })
  }

  getSnapElements(connectedElements) {
    const sketch = this.viewport.document.activeSketch
    const tool = this.viewport.activeTool
    const snapExclusions = new Set(tool.snapExclusions())
    let sketchElements = [
      ...sketch.elements,
      ...sketch.projections.map(projection => projection.geometry()).filter(Boolean),
    ].filter(elem => !snapExclusions.has(elem) && tool.acceptsSnapElement(elem))
    if(this.viewport.activeHandle) {
      sketchElements = sketchElements.filter(elem => !connectedElements.has(elem))
    }
    return sketchElements
  }

  getSnapPointTargets(elements, connectedElements) {
    const sketch = this.viewport.document.activeSketch
    const points = elements.flatMap(elem =>
      elem.snapPoints().map(point => ({
        point,
        elem,
        index: elem.handles().findIndex(handle => handle.almost(point)),
        midpoint: elem instanceof Line && point.almost(elem.midpoint()),
      }))
    )
    const origin = sketch.originPoint()
    const originHelper = sketch.origin()
    const originConstrained = originHelper && connectedElements.has(originHelper)
    const includeOrigin = origin && !originConstrained &&
      this.viewport.activeTool.acceptsSnapElement(originHelper)
    return includeOrigin ? [{ point: origin, elem: originHelper, index: 0 }, ...points] : points
  }

  catchSnapTargets(localVec, coords, rememberGuides) {
    const connectedElements = this.getActiveHandleConnections()
    const elements = this.getSnapElements(connectedElements)
    const canReach = this.activeHandleReachability()
    let pointDist = Infinity
    let pointTarget
    let blockedPointDist = Infinity
    let midpointTarget
    let guidePointDist = Infinity
    let guidePointTarget
    this.getSnapPointTargets(elements, connectedElements).forEach(target => {
      const dist = this.viewport.renderer.toScreen(target.point.clone().applyMatrix4(this.planeTransform)).distanceTo(coords)
      const reachable = canReach(target.point)
      if(rememberGuides && dist < snapDistance && dist < guidePointDist) {
        guidePointDist = dist
        guidePointTarget = target
      }
      if(target.index != -1 && dist < snapDistance) {
        if(reachable && dist < pointDist) {
          pointDist = dist
          pointTarget = target
        } else if(!reachable && dist < blockedPointDist) {
          blockedPointDist = dist
        }
      }
      if(reachable && target.midpoint && dist < snapDistance && (!midpointTarget || dist < midpointTarget.distance)) {
        midpointTarget = { ...target, distance: dist }
      }
    })

    // Handles and the sketch origin always take precedence over curves.
    if(pointTarget && pointDist < blockedPointDist) {
      return rememberGuides ? this.rememberSnapPoint(pointTarget) : { pointTarget }
    }
    if(blockedPointDist < Infinity) return

    // An intersection near the pointer must be near both participating
    // curves. Reject the rest before invoking OpenCascade's substantially
    // more expensive curve/curve intersection routine.
    const nearbyCurves = elements
      .filter(supportsPointOnCurveConstraint)
      .map(elem => {
        const point = closestPointOnCurve(elem, localVec)
        if(!point || !canReach(point)) return
        const distance = this.viewport.renderer
          .toScreen(point.clone().applyMatrix4(this.planeTransform))
          .distanceTo(coords)
        return { elem, point, distance }
      })
      .filter(target => target && target.distance < snapDistance)

    let intersectionTarget
    nearbyCurves.forEach((target, index, curves) =>
      curves.slice(index + 1).forEach(otherTarget => {
        target.elem.intersect([otherTarget.elem]).forEach(ocPoint => {
          const point = vecFromOc(ocPoint)
          if(!canReach(point)) return
          const dist = this.viewport.renderer.toScreen(point.clone().applyMatrix4(this.planeTransform)).distanceTo(coords)
          if(dist < snapDistance && (!intersectionTarget || dist < intersectionTarget.distance)) {
            intersectionTarget = { curves: [target.elem, otherTarget.elem], point, distance: dist }
          }
        })
      })
    )
    if(intersectionTarget) {
      if(rememberGuides) this.rememberSnapPoint({ point: intersectionTarget.point })
      return intersectionTarget
    }

    if(midpointTarget) return rememberGuides ? this.rememberSnapPoint(midpointTarget) : { pointTarget: midpointTarget }

    const curveTarget = nearbyCurves.reduce((closest, target) =>
      !closest || target.distance < closest.distance ? target : closest
    , null)

    if(curveTarget) return { elem: curveTarget.elem, point: curveTarget.point }

    if(rememberGuides && guidePointTarget) return this.rememberSnapPoint(guidePointTarget)
  }

  rememberSnapPoint(pointTarget) {
    const reference = {
      ...pointTarget,
      point: pointTarget.point.clone(),
    }
    const previous = this.lastSnaps[0]
    const sameTarget = previous && (
      (previous.elem && reference.elem && previous.elem == reference.elem &&
        previous.index == reference.index && previous.midpoint == reference.midpoint) ||
      this.resolveSnapReference(previous).equals(this.resolveSnapReference(reference))
    )
    if(!sameTarget) {
      this.lastSnaps.unshift(reference)
      if(this.lastSnaps.length > maxSnapReferences) this.lastSnaps.pop()
    }
    return { pointTarget }
  }

  resolveSnapReference(reference) {
    let elem = reference.elem
    let index = reference.index
    if(elem && elem.dissolvedTo) {
      index = elem.dissolvedTo.index
      elem = elem.dissolvedTo.elem
    }
    if(reference.midpoint && elem instanceof Line) return elem.midpoint()
    const handle = elem && index != -1 && elem.handles && elem.handles()[index]
    return (handle || reference.point).clone()
  }

  getGuideSnapPoints() {
    const activePoints = this.viewport.activeTool.guideSnapPoints()
    const origin = this.viewport.document.activeSketch.originPoint()
    const remembered = this.lastSnaps.map(reference => this.resolveSnapReference(reference))
    return [...activePoints, ...(origin ? [origin] : []), ...remembered].filter((point, index, points) =>
      points.findIndex(other => other.equals(point)) === index
    )
  }

  guideScreenPoint(point) {
    return this.viewport.renderer.toScreen(point.clone().applyMatrix4(this.planeTransform))
  }

  setSnapAnchor(point, id) {
    const vec = point.clone().applyMatrix4(this.planeTransform)
    this.snapAnchor = { type: 'snap', pos: this.viewport.renderer.toScreen(vec), vec, id }
  }

  closestTangentSnap(localVec, coords, target) {
    const start = this.viewport.activeTool.tangentSnapStart()
    if(!start || !target) return
    const curves = [target.elem, target.pointTarget?.elem, ...(target.curves || [])]
      .filter(elem => elem instanceof Circle || elem instanceof Arc)
    let closest
    curves.forEach(elem => {
      const center = elem.center()
      const delta = start.clone().sub(center)
      const distanceSquared = delta.lengthSq()
      const radiusSquared = elem.radius * elem.radius
      if(distanceSquared <= radiusSquared) return
      const base = center.clone().addScaledVector(delta, radiusSquared / distanceSquared)
      const offset = new THREE.Vector3(-delta.y, delta.x, 0)
        .multiplyScalar(elem.radius * Math.sqrt(distanceSquared - radiusSquared) / distanceSquared)
      for(const sign of [-1, 1]) {
        const point = base.clone().addScaledVector(offset, sign)
        if(elem instanceof Arc && !closestPointOnCurve(elem, point).almost(point)) continue
        const direction = point.clone().sub(start).normalize()
        if(localVec.clone().sub(start).normalize().dot(direction) < Math.cos(lineSnapAngle)) continue
        const distance = this.guideScreenPoint(point).distanceTo(coords)
        if(distance >= snapDistance || (closest && distance >= closest.distance)) continue
        closest = { elem, start, point, distance }
      }
    })
    return closest
  }

  showTangentSnap(tangent) {
    this.snapped.tangent = tangent.elem
    this.guides.push({
      id: 'tangent-' + tangent.elem.id,
      start: this.guideScreenPoint(tangent.start),
      end: this.guideScreenPoint(tangent.point),
    })
  }

  getSnapGuides(localVec, coords, snapTarget) {
    const guides = []
    const context = this.viewport.activeTool.lineSnapContext()
    if(context) {
      const { start, perpendicularLines } = context
      const delta = localVec.clone().sub(start)
      const elements = this.getSnapElements(this.getActiveHandleConnections())
      const candidates = [
        ...new Set([...perpendicularLines, ...snapTargetLines(snapTarget)]),
      ].map(elem => ({ elem, perpendicular: true })).concat(
        this.lastLines.map(elem => ({ elem, perpendicular: false }))
      )
      if(!delta.lengthSq().almost(0.0)) candidates.forEach(({ elem, perpendicular }) => {
        if(!elements.includes(elem)) return
        const direction = elem.direction()
        if(direction.lengthSq().almost(0.0)) return
        direction.normalize()
        if(perpendicular) direction.set(-direction.y, direction.x, 0)
        if(Math.abs(delta.clone().normalize().dot(direction)) < Math.cos(lineSnapAngle)) return
        guides.push({ start, direction, elem, perpendicular })
      })
    }
    this.getGuideSnapPoints().forEach(start => ['x', 'y'].forEach(axis => {
      guides.push({ start, axis, direction: new THREE.Vector3(axis == 'y' ? 1 : 0, axis == 'x' ? 1 : 0, 0) })
    }))
    return guides.map(guide => {
      const point = projectToGuide(localVec, guide)
      return { ...guide, point, distance: this.guideScreenPoint(point).distanceTo(coords) }
    }).filter(guide => guide.distance < snapDistance)
  }

  guidesAtPoint(guides, point) {
    const matched = []
    guides.forEach(guide => {
      if(!projectToGuide(point, guide).almost(point)) return
      // Collinear guides express the same snap. Prefer a line relation to
      // an axis relation, and keep at most two independent directions.
      if(matched.length == 2 || matched.some(other => cross2d(other.direction, guide.direction).almost(0.0))) return
      if(guide.elem && point.almost(guide.start)) return
      matched.push(guide)
    })
    return matched
  }

  closestGuideSnap(guides, coords, curve) {
    let closest
    const consider = point => {
      const distance = this.guideScreenPoint(point).distanceTo(coords)
      const matched = this.guidesAtPoint(guides, point)
      // Two nearby guides may meet slightly farther from the pointer than
      // either individual guide (as with horizontal + vertical snapping).
      const limit = snapDistance * (matched.length > 1 ? Math.SQRT2 : 1)
      if(!matched.length || distance >= limit) return
      if(closest && (matched.length < closest.guides.length ||
        (matched.length == closest.guides.length && distance >= closest.distance))) return
      closest = { point, distance, guides: matched }
    }
    guides.forEach((guide, index) => {
      if(curve) {
        curveGuideIntersections(curve, guide).forEach(consider)
      } else {
        consider(guide.point)
        guides.slice(index + 1).forEach(other => {
          const point = guideIntersection(guide, other)
          if(point) consider(point)
        })
      }
    })
    return closest
  }

  showSnapGuides(guides, point) {
    guides.forEach(guide => {
      const start = this.guideScreenPoint(guide.start)
      if(guide.elem) {
        const [first, second] = guide.elem.endpoints()
        this.guides.push({ id: 'line-reference-' + guide.elem.id, start: this.guideScreenPoint(first), end: this.guideScreenPoint(second) })
        this.snapped.lineRelation = { elem: guide.elem, perpendicular: guide.perpendicular }
      } else {
        this.snapped[guide.axis] = guide.start
      }
      this.guides.push({
        id: guide.elem ? 'line-direction-' + guide.elem.id : guide.axis + start.x + '-' + start.y,
        start,
        end: this.guideScreenPoint(point),
      })
    })
  }

  snapToGuides(vec, snapToGuides, snapToPoints) {
    if(!vec) return

    const localTransform = this.planeTransform.clone().invert()
    const localVec = vec.clone().applyMatrix4(localTransform)

    if(!(snapToGuides || snapToPoints)) return localVec

    const snapTarget = snapToPoints &&
      this.catchSnapTargets(localVec, this.viewport.renderer.toScreen(vec), snapToGuides)

    if(snapToGuides && snapTarget) {
      snapTargetLines(snapTarget).forEach(line => {
        this.lastLines = [line, ...this.lastLines.filter(other => other != line)].slice(0, maxSnapReferences)
      })
    }

    const screenVec = this.viewport.renderer.toScreen(vec)
    const guides = snapToGuides ? this.getSnapGuides(localVec, screenVec, snapTarget) : []
    const tangent = snapToGuides && this.closestTangentSnap(localVec, screenVec, snapTarget)

    if(snapTarget && snapTarget.curves) {
      const snapVec = snapTarget.point
      this.snapped = { intersection: snapTarget.curves, point: snapVec }
      if(tangent && tangent.point.almost(snapVec)) this.showTangentSnap(tangent)
      this.showSnapGuides(this.guidesAtPoint(guides, snapVec), snapVec)
      this.setSnapAnchor(snapVec, 'intersection-' + snapTarget.curves.map(curve => curve.id).join('-'))
      return snapVec
    }

    if(snapTarget && snapTarget.pointTarget) {
      const pointTarget = snapTarget.pointTarget
      const snapVec = pointTarget.point.clone()
      this.snapped = { x: snapVec, y: snapVec, pointTarget }
      if(tangent && tangent.point.almost(snapVec)) this.showTangentSnap(tangent)
      this.showSnapGuides(this.guidesAtPoint(guides, snapVec), snapVec)
      this.snapped.x = snapVec
      this.snapped.y = snapVec
      this.setSnapAnchor(snapVec, 'point-' + (pointTarget.elem?.id || 'origin') + '-' + (pointTarget.index ?? 0))
      return snapVec
    }

    if(snapTarget && snapTarget.elem) {
      if(tangent) {
        this.snapped = { curve: snapTarget.elem }
        this.showTangentSnap(tangent)
        this.showSnapGuides(this.guidesAtPoint(guides, tangent.point), tangent.point)
        this.setSnapAnchor(tangent.point, 'tangent-' + tangent.elem.id)
        return tangent.point
      }
      const guideSnap = this.closestGuideSnap(guides, screenVec, snapTarget.elem)
      const snapVec = guideSnap ? guideSnap.point : snapTarget.point
      this.snapped = { curve: snapTarget.elem }
      if(guideSnap) this.showSnapGuides(guideSnap.guides, snapVec)
      this.setSnapAnchor(snapVec, 'curve-' + snapTarget.elem.id)
      return snapVec
    }

    const guideSnap = this.closestGuideSnap(guides, screenVec)
    if(!guideSnap) return localVec
    const snapVec = guideSnap.point
    this.showSnapGuides(guideSnap.guides, snapVec)
    if(guideSnap.guides.length > 1) {
      this.setSnapAnchor(snapVec, '' + snapVec.x + snapVec.y + snapVec.z)
    }
    return snapVec
  }
}
