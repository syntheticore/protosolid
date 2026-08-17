import * as THREE from 'three'
import { Line, Circle, Arc, Spline, supportsPointOnCurveConstraint } from './core/geom2d.js'
import { vecFromOc } from './core/utils.js'
import { CoincidentConstraint, FixConstraint, HorVertConstraint } from './core/sketch.js'

const snapDistance = 14 // px
const maxSnapReferences = 5
const fullTurn = Math.PI * 2.0

function positiveAngle(angle) {
  return ((angle % fullTurn) + fullTurn) % fullTurn
}

function closestPointOnCurve(elem, point) {
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

function curveGuideIntersections(elem, guidePoint, axis) {
  // Curve snapping is exact for splines, but guide/spline intersections
  // require a separate unbounded-line projection. Leave guide snapping out
  // rather than falling through to the circle-specific center/radius path.
  if(elem instanceof Spline) return []

  if(elem instanceof Line) {
    const [start, end] = elem.endpoints()
    const delta = end.clone().sub(start)
    const denominator = axis == 'x' ? delta.x : delta.y
    if(denominator.almost(0.0)) return []
    const value = axis == 'x' ? guidePoint.x : guidePoint.y
    const parameter = (value - (axis == 'x' ? start.x : start.y)) / denominator
    if(parameter < 0.0 || parameter > 1.0) return []
    return [start.clone().addScaledVector(delta, parameter)]
  }

  const center = elem.center()
  const value = axis == 'x' ? guidePoint.x : guidePoint.y
  const offset = value - (axis == 'x' ? center.x : center.y)
  const remainder = elem.radius * elem.radius - offset * offset
  if(remainder < 0.0) return []
  const span = Math.sqrt(Math.max(0.0, remainder))
  const points = [-span, span].map(other => axis == 'x' ?
    new THREE.Vector3(value, center.y + other, center.z)
    :
    new THREE.Vector3(center.x + other, value, center.z)
  )
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
    this.snapAnchor = null
    this.planeTransform = new THREE.Matrix4()
  }

  reset() {
    this.guides = []
    this.snapAnchor = null
    this.lastSnaps = []
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
      if(curve == sketch.origin() || curve.projection) return true
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
    let sketchElements = [...sketch.elements]
    // Filter out sketch element actively being drawn
    const tool = this.viewport.activeTool
    if(tool.curve) sketchElements.pop()
    sketchElements.push(...sketch.projections.map(projection => projection.geometry()).filter(Boolean))
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
    return origin && !originConstrained ? [{ point: origin, origin: true }, ...points] : points
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
      if((target.origin || target.index != -1) && dist < snapDistance) {
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
      (previous.origin && reference.origin) ||
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
    if(reference.origin) return this.viewport.document.activeSketch.originPoint()
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

  curveGuideSnap(curve, guideSnapPoints, coords) {
    let closest
    guideSnapPoints.forEach(guidePoint => ['x', 'y'].forEach(axis => {
      curveGuideIntersections(curve, guidePoint, axis).forEach(point => {
        const worldPoint = point.clone().applyMatrix4(this.planeTransform)
        const distance = this.viewport.renderer.toScreen(worldPoint).distanceTo(coords)
        if(distance >= snapDistance || (closest && distance >= closest.distance)) return
        closest = { point, distance, [axis]: guidePoint }
      })
    }))
    return closest
  }

  snapToGuides(vec, snapToGuides, snapToPoints) {
    if(!vec) return

    const localTransform = this.planeTransform.clone().invert()
    const localVec = vec.clone().applyMatrix4(localTransform)

    if(!(snapToGuides || snapToPoints)) return localVec

    const snapTarget = snapToPoints &&
      this.catchSnapTargets(localVec, this.viewport.renderer.toScreen(vec), snapToGuides)

    const screenVec = this.viewport.renderer.toScreen(vec)
    const guideSnapPoints = snapToGuides ? this.getGuideSnapPoints() : []

    if(snapTarget && snapTarget.curves) {
      const snapVec = snapTarget.point
      const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
      this.snapped = { intersection: snapTarget.curves, point: snapVec }
      this.snapAnchor = {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(worldSnapVec),
        vec: worldSnapVec,
        id: 'intersection-' + snapTarget.curves.map(curve => curve.id).join('-'),
      }
      return snapVec
    }

    if(snapTarget && snapTarget.pointTarget) {
      const pointTarget = snapTarget.pointTarget
      const snapVec = pointTarget.point.clone()
      const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
      this.snapped = { x: snapVec, y: snapVec, pointTarget }
      this.snapAnchor = {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(worldSnapVec),
        vec: worldSnapVec,
        id: 'point-' + (pointTarget.elem?.id || 'origin') + '-' + (pointTarget.index ?? 0),
      }
      return snapVec
    }

    if(snapTarget && snapTarget.elem) {
      const guideSnap = snapToGuides && this.curveGuideSnap(snapTarget.elem, guideSnapPoints, screenVec)
      const snapVec = guideSnap ? guideSnap.point : snapTarget.point
      const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
      this.snapped = { curve: snapTarget.elem, ...guideSnap }
      delete this.snapped.point
      delete this.snapped.distance
      this.snapAnchor = {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(worldSnapVec),
        vec: worldSnapVec,
        id: 'curve-' + snapTarget.elem.id,
      }
      if(guideSnap) {
        const guidePoint = guideSnap.x || guideSnap.y
        const start = this.viewport.renderer.toScreen(guidePoint.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: (guideSnap.x ? 'v' : 'h') + start.x + start.y,
          start,
          end: this.viewport.renderer.toScreen(worldSnapVec),
        })
      }
      return snapVec
    }

    const closestGuide = axis => guideSnapPoints.reduce((closest, snap) => {
      const testSnap = snap.clone()
      if(axis == 'x') testSnap.setY(localVec.y)
      else testSnap.setX(localVec.x)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const distance = screenVec.distanceTo(this.viewport.renderer.toScreen(testSnap))
      return distance < snapDistance && (!closest || distance < closest.distance) ?
        { point: snap, distance }
        :
        closest
    }, null)?.point
    const snapX = closestGuide('x')
    const snapY = closestGuide('y')
    this.snapped = { x: snapX, y: snapY }
    // const snapVec = new THREE.Vector3(
    //   snapX ? snapX.clone().applyMatrix4(localTransform).x : localVec.x,
    //   snapY ? snapY.clone().applyMatrix4(localTransform).y : localVec.y,
    //   localVec.z
    // ) //XXX z-imprecision
    const snapVec = new THREE.Vector3(
      snapX ? snapX.x : localVec.x,
      snapY ? snapY.y : localVec.y,
      localVec.z
    )
    const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
    const screenSnapVec = this.viewport.renderer.toScreen(worldSnapVec)
    if(snapToGuides) {
      if(snapX) {
        const start = this.viewport.renderer.toScreen(snapX.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: 'v' + start.x + start.y,
          start,
          end: screenSnapVec,
        })
      }
      if(snapY) {
        const start = this.viewport.renderer.toScreen(snapY.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: 'h' + start.x + start.y,
          start,
          end: screenSnapVec,
        })
      }
    }
    if(snapX && snapY) {
      this.snapAnchor = {
        type: 'snap',
        pos: screenSnapVec,
        vec: worldSnapVec,
        id: '' + snapVec.x + snapVec.y + snapVec.z,
      }
      if(snapX === snapY && snapToPoints) return snapX//.clone().applyMatrix4(localTransform)
    } else {
      this.snapAnchor = null
    }

    if((snapX || snapY) && snapToGuides) return snapVec

    return localVec
  }
}
