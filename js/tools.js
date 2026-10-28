import * as THREE from 'three'
import { markRaw, toRaw } from 'vue'

// import {
//   vecToThree,
//   matrix2three,
//   matrixFromThree,
//   rotationFromNormal
// } from './utils.js'

import {
  SketchElement,
  Line,
  Circle,
  Arc,
  Spline,
  OffsetCurve,
  SketchPoint,
} from './core/geom2d.js'

import {
  Edge,
  Face,
  Vertex,
  Profile,
  Solid,
} from './core/geom3d.js'
import { getViewportTessellationTolerance, shapex as sx, point2, modelTolerance } from './core/shapex.js'
import { PlaneHelper, PointHelper } from './core/helpers.js'
import { ScreenPointObject } from './three/helper-objects.js'

import {
  CoincidentConstraint,
  TouchConstraint,
  MidpointConstraint,
  PerpendicularConstraint,
  HorVertConstraint,
  ParallelConstraint,
  EqualConstraint,
  ConcentricConstraint,
  TangentConstraint,
  SymmetricConstraint,
  FixConstraint,
  Dimension,
  OffsetConstraint,
  ProfileOffsetConstraint,
  Projection,
  Intersection,
  ProjectedPoint,
  SketchOrigin,
  ElemRef,
} from './core/sketch.js'
import {
  solveAssembly,
  worldTransform,
} from './core/assembly.js'
import { EPSILON } from './core/utils.js'
import { supportsFillet, curveFillet, trimToPoint } from './core/sketch-fillet.js'
import { pasteSketchSelection } from './sketch-clipboard.js'
import materials, { DRAFT_DIRECTIONS } from './materials.js'
import { supportsOffset, connectedOffsetChain, profileOffsetGroups, nearestOffsetFrame, buildOffsetCurves } from './core/sketch-offset.js'

const dragThreshold = 2
const tangentSnapAngle = THREE.MathUtils.degToRad(7.5)

class Tool {
  static icon = 'bullseye'

  constructor(component, viewport) {
    this.component = component
    this.viewport = viewport
    this.snapToGuides = false
    this.snapToPoints = false
    this.localSpace = false
    this.cursor = 'auto'
  }

  click(vec, coords) {}

  mouseDown(vec, coords) {
    this.lastCoords = coords
  }

  mouseUp(vec, coords) {
    const lastCoords = this.lastCoords
    this.lastCoords = null
    if((!vec && !this.probesSurface) || !lastCoords ||
      coords.distanceTo(lastCoords) > dragThreshold) return this.viewport.renderer.render()
    this.click(vec, coords)
    // window.gc && window.gc()
  }

  cancelPointer() { this.lastCoords = null }

  mouseMove(vec, coords) {}

  guideSnapPoints() { return [] }

  lineSnapContext() { return null }

  tangentSnapStart() { return null }

  // Numeric fields shown next to the cursor while drawing:
  // [{ key, label, dimension, value }]
  inputFields() { return [] }

  snapExclusions() { return this.curve ? [this.curve] : [] }

  historyState() {
    if(!this.sketch) return
    const previews = new Set([
      this.curve, this.arc, ...(this.lines || []),
      ...(this.elements || []), ...(this.curves || []),
    ])
    const state = this.sketch.dump()
    state.elements = state.elements.filter(elem => !previews.has(elem))
    state.constraints = state.constraints.filter(constraint =>
      !constraint.items.some(item => previews.has(item.curve())))
    return state
  }

  acceptsSnapElement(_elem) { return true }

  dispose() {}
}

function captureSnap(snapper) {
  const { curve, pointTarget, intersection, point, x, y, lineRelation, tangent } = snapper.snapped || {}
  if(!(curve || pointTarget || intersection || x || y || lineRelation)) return
  return { ...pointTarget, curve, intersection, point, x, y, lineRelation, tangent }
}

function isConstrainablePoint(elem, index) {
  if(elem instanceof SketchOrigin) return index == 0
  if(elem instanceof SketchPoint) return index == 0
  if(elem instanceof ProjectedPoint) return index == 0
  if(elem instanceof Line) return index == 0 || index == 1
  if(elem instanceof Circle) return index == 0
  if(elem instanceof Arc) return index >= 0 && index <= 2
  if(elem instanceof Spline) return index == 0 || index == elem.handles().length - 1
  if(elem instanceof OffsetCurve) return index == 0 || index == 1
  return false
}

function isTouchPoint(elem, index) {
  return isConstrainablePoint(elem, index) && !(elem instanceof Arc && index == 0)
}

function pointTargetsAtPosition(sketch, position, excludedElem) {
  if(!position) return []
  const candidates = [
    ...sketch.elements,
    ...sketch.projectedElements(),
  ]
  return candidates.flatMap(candidate =>
    toRaw(candidate) == toRaw(excludedElem) || !candidate.handles ? [] : candidate.handles().map((point, index) => ({
      elem: candidate,
      index,
      point,
    }))
  ).filter(target =>
    isConstrainablePoint(target.elem, target.index) && target.point.almost(position)
  )
}

function constrainCoincidentPoints(sketch, elem, index, targetElem, targetIndex, preservePoints=false) {
  if(!preservePoints && elem instanceof SketchPoint && sketch.dissolvePoint(elem, targetElem, targetIndex)) return elem
  if(!preservePoints && targetElem instanceof SketchPoint && sketch.dissolvePoint(targetElem, elem, index)) return targetElem
  return sketch.addConstraint(
    new CoincidentConstraint(new ElemRef(elem, index), new ElemRef(targetElem, targetIndex))
  )
}

function addCapturedSnapConstraint(sketch, snap, elem, index, preservePoints=false) {
  if(!isConstrainablePoint(elem, index)) return

  // Explicit points retain priority even if another overlapping target won
  // the screen-space snap tie.
  const position = elem.handles()[index]
  const positionTargets = pointTargetsAtPosition(sketch, position, elem)
  const explicitTarget = positionTargets.find(target => target.elem instanceof SketchPoint)
  if(explicitTarget) return constrainCoincidentPoints(
    sketch, elem, index, explicitTarget.elem, explicitTarget.index, preservePoints
  )

  if(snap && snap.intersection) {
    const point = sketch.findOrCreateIntersectionPoint(snap.intersection, snap.point)
    if(point == elem) return point
    return constrainCoincidentPoints(sketch, elem, index, point, 0, preservePoints)
  }
  if(snap && snap.midpoint && snap.elem instanceof Line && snap.elem != elem) return sketch.addConstraint(
    new MidpointConstraint(new ElemRef(elem, index), snap.elem)
  )
  if(snap && snap.curve && snap.curve != elem) {
    if(!isTouchPoint(elem, index)) return
    const touch = sketch.addConstraint(
      new TouchConstraint(new ElemRef(elem, index), snap.curve)
    )
    addCapturedGuideConstraints(sketch, snap, elem, index)
    return touch
  }
  if(snap && snap.elem && snap.elem != elem && snap.index != -1) {
    if(snap.elem instanceof SketchPoint && snap.elem.dissolvedTo) {
      const target = snap.elem.dissolvedTo
      if(target.elem == elem && target.index == index) return
      return sketch.addConstraint(
        new CoincidentConstraint(new ElemRef(elem, index), new ElemRef(target.elem, target.index))
      )
    }
    return constrainCoincidentPoints(sketch, elem, index, snap.elem, snap.index, preservePoints)
  }

  // Only use another coincident handle as a positional fallback when no
  // captured target was available. Otherwise an already-connected endpoint
  // can mask the point the user just snapped to.
  const positionTarget = positionTargets[0]
  if(positionTarget) return constrainCoincidentPoints(
    sketch, elem, index, positionTarget.elem, positionTarget.index, preservePoints
  )
}

function addCapturedGuideConstraints(sketch, snap, elem, index) {
  ;[['x', snap.x], ['y', snap.y]].forEach(([axis, guidePoint]) => {
    if(!guidePoint) return
    const ref = new ElemRef(elem, index)
    let target
    if(guidePoint.almost(sketch.originPoint())) {
      target = new ElemRef(sketch.origin())
    } else {
      const candidates = [
        ...sketch.elements,
        ...sketch.projectedElements(),
      ]
      for(const candidate of candidates) {
        const targetIndex = candidate.handles().findIndex(handle => handle.almost(guidePoint))
        if(targetIndex == -1) continue
        if(candidate == elem) {
          if(elem instanceof Line && targetIndex != index) target = elem
        } else if(isConstrainablePoint(candidate, targetIndex)) {
          target = new ElemRef(candidate, targetIndex)
        }
        if(target) break
      }
    }
    if(!target) return
    const constraint = new HorVertConstraint(ref, ...(target instanceof ElemRef ? [target] : []))
    constraint.isVertical = axis == 'x'
    sketch.addConstraint(constraint)
  })
}

// Apply locked length and angle (degrees from the sketch x axis) to a point
// relative to start.
function polarPoint(start, point, locks) {
  const delta = point.clone().sub(start).setZ(0)
  const direction = locks.angle === undefined ?
    delta.clone().normalize()
    :
    new THREE.Vector3(
      Math.cos(THREE.MathUtils.degToRad(locks.angle)),
      Math.sin(THREE.MathUtils.degToRad(locks.angle)),
      0,
    )
  if(direction.lengthSq() < EPSILON) direction.set(1, 0, 0)
  const length = locks.length ?? (locks.angle === undefined ? delta.length() : delta.dot(direction))
  return start.clone().addScaledVector(direction, length)
}

function radialPoint(center, point, radius) {
  const direction = point.clone().sub(center).setZ(0).normalize()
  if(direction.lengthSq() < EPSILON) direction.set(1, 0, 0)
  return center.clone().addScaledVector(direction, radius)
}

function polarFields(start, end) {
  const delta = end.clone().sub(start)
  const angle = THREE.MathUtils.radToDeg(Math.atan2(delta.y, delta.x))
  return [
    { key: 'length', label: 'L', dimension: 'length', value: delta.length() },
    { key: 'angle', label: '∠', dimension: 'angle', value: (angle + 360) % 360 },
  ]
}

// Dimension label position beside a line, facing away from center if given.
function dimensionPosition(line, offset, center) {
  const midpoint = line.midpoint()
  const direction = line.direction().normalize()
  const normal = new THREE.Vector3(-direction.y, direction.x, 0)
  if(center && normal.dot(midpoint.clone().sub(center)) < 0) normal.negate()
  return midpoint.addScaledVector(normal, offset)
}

function addInferredTangentConstraint(sketch, line, snap, endpointIndex=1) {
  const target = snap && (snap.curve || (snap.elem instanceof Arc && snap.index > 0 && snap.elem))
  if(!(target instanceof Circle || target instanceof Arc)) return
  const direction = line.direction()
  if(direction.lengthSq().almost(0.0)) return
  const endpoint = line.endpoints()[endpointIndex]
  const radial = endpoint.clone().sub(target.center())
  const radialAlignment = Math.abs(direction.normalize().dot(radial.normalize()))
  if(radialAlignment > Math.sin(tangentSnapAngle)) return

  sketch.addConstraint(new TangentConstraint(line, target))
}


export class DummyTool extends Tool {
  mouseMove(vec, coords) {
    // this.viewport.renderer.render()
  }

  async click(vec, coords) {
    this.viewport.document.selection.clear()
  }
}


export class SimulationProbeTool extends Tool {
  constructor(component, viewport) {
    super(component, viewport)
    this.cursor = 'crosshair'
    this.probesSurface = true
  }

  mouseMove(_vec, coords) {
    const simulation = this.viewport.document.activeSimulation
    const hit = this.viewport.renderer.hitTest(coords).find(intersection => {
      const face = intersection.object.alcObject
      return face?.solid?.component && simulation?.result &&
        simulation.result.componentId == face.solid.component.id &&
        simulation.result.solidId == face.solid.id
    })
    if(!hit) {
      this.viewport.thermalProbe = null
      return
    }

    const component = hit.object.alcObject.solid.component
    const position = hit.point.clone().applyMatrix4(worldTransform(component).invert())
    const label = this.label(simulation, position)
    this.viewport.thermalProbe = label === undefined ? null : {
      position: coords,
      label,
    }
  }

  dispose() {
    this.viewport.thermalProbe = null
  }
}


export class ThermalProbeTool extends SimulationProbeTool {
  label(simulation, position) {
    const temperature = simulation.temperatureAt(position)
    if(temperature !== undefined) return `${temperature.toFixed(1)} °C`
  }
}


export class StaticProbeTool extends SimulationProbeTool {
  label(simulation, position) {
    const sample = simulation.sampleAt(position)
    if(sample) return `${sample.stress.toPrecision(3)} MPa · ${sample.displacement.length().toPrecision(3)} mm displacement`
  }
}


export class FluidProbeTool extends SimulationProbeTool {
  label(simulation, position) {
    const sample = simulation.sampleAt(position)
    if(sample) return `${sample.speed.toFixed(3)} m/s · ${sample.pressure.toFixed(2)} Pa gauge`
  }
}


export class DiagnosticShadingTool extends Tool {
  static icon = 'palette'
  static hasOptions = true

  constructor(component, viewport) {
    super(component, viewport)
    this.diagnosticMode = 'zebra'
    this.draftDirection = 'top'
    this.zebraFrequency = 16
    this.probesSurface = true
    this.settings = {
      diagnosticMode: {
        title: 'Analysis',
        type: 'select',
        options: {
          zebra: { title: 'Zebra Stripes', icon: 'bars' },
          draft: { title: 'Draft Angle', icon: 'angle-double-up' },
        },
      },
      draftDirection: {
        title: 'Direction',
        type: 'enum',
        options: {
          top: { title: 'Top', icon: 'arrow-down' },
          front: { title: 'Front', icon: 'arrow-up' },
          side: { title: 'Side', icon: 'arrow-left' },
        },
        visible: tool => tool.diagnosticMode == 'draft',
      },
      zebraFrequency: {
        title: 'Frequency',
        type: 'integer',
        min: 1,
        max: 64,
        focus: true,
        visible: tool => tool.diagnosticMode == 'zebra',
      },
    }
  }

  setOption(key, value) {
    if(!this.settings[key]) return
    this[key] = value
    this.viewport.thermalProbe = null
    this.viewport.renderer.render()
  }

  mouseMove(_vec, coords) {
    if(this.diagnosticMode != 'draft') {
      this.viewport.thermalProbe = null
      return
    }
    const hit = this.viewport.renderer.hitTest(coords).find(intersection => intersection.object.alcObject?.solid)
    if(!hit) {
      this.viewport.thermalProbe = null
      return
    }

    const normal = hit.normal.clone().applyNormalMatrix(
      new THREE.Matrix3().getNormalMatrix(hit.object.matrixWorld)
    ).normalize()
    const direction = new THREE.Vector3(...DRAFT_DIRECTIONS[this.draftDirection])
    const angle = THREE.MathUtils.radToDeg(Math.asin(THREE.MathUtils.clamp(normal.dot(direction), -1, 1)))
    this.viewport.thermalProbe = {
      position: coords,
      label: `Draft ${angle.toFixed(1)}°`,
    }
  }

  dispose() {
    this.viewport.thermalProbe = null
  }
}


export class InterferenceTool extends Tool {
  static icon = 'traffic-light'
  static hasOptions = true

  constructor(component, viewport) {
    super(component, viewport)
    this.interferenceMode = true
    this.collisionVolumes = []
    this.collisionPreview = markRaw(new THREE.Group())
    this.settings = {
      collisionVolumes: {
        title: 'Collisions',
        type: 'info-list',
      },
    }

    const parts = component.getChildren()
      .filter(part => part.compound.solids().length)
      .map(part => ({
        component: part,
        bounds: this.worldBounds(part),
      }))
    const overlaps = []

    for(let i = 0; i < parts.length; i++) {
      for(let j = i + 1; j < parts.length; j++) {
        if(!parts[i].bounds.intersectsBox(parts[j].bounds)) continue

        const first = this.worldCompound(parts[i])
        const second = this.worldCompound(parts[j])
        const overlap = first.boolean(second, 'intersect')
        const volume = overlap.volume()
        if(!overlap.solids().length || !(volume > 0.000001)) continue
        overlaps.push({ overlap, volume })
      }
    }

    this.collisionVolumes = overlaps.map(({ volume }, index) => ({
      label: `${index + 1}`,
      value: `${(volume / 1000).toFixed(2)} cm³`,
    }))
    overlaps.forEach(({ overlap }) => this.collisionPreview.add(
      viewport.renderer.convertMesh(overlap.tesselate(), materials.collisionSurface)
    ))
    viewport.renderer.add(this.collisionPreview)
    viewport.renderer.render()
  }

  worldBounds(part) {
    const bounds = new THREE.Box3()
    const positions = part.compound.tesselate().positions
    const transform = worldTransform(part)
    for(let i = 0; i < positions.length; i += 3) {
      bounds.expandByPoint(
        new THREE.Vector3(positions[i], positions[i + 1], positions[i + 2])
          .applyMatrix4(transform)
      )
    }
    // Keep this broad-phase check conservative around tessellated curves.
    return bounds.expandByScalar(getViewportTessellationTolerance())
  }

  worldCompound(part) {
    if(!part.worldCompound) {
      part.worldCompound = part.component.compound.transform(worldTransform(part.component))
    }
    return part.worldCompound
  }

  dispose() {
    this.viewport.renderer.remove(this.collisionPreview)
    this.collisionPreview = null
    this.viewport.renderer.render()
  }
}


class HighlightTool extends Tool {
  constructor(component, viewport, selectors) {
    super(component, viewport)
    this.setSelectors(selectors)
  }

  setSelectors(selectors) {
    this.selectors = selectors
    this.realSelectors = selectors.map(selector => ['solid', 'component'].includes(selector) ? 'face' : selector )
  }

  orderItems(items) { return items }

  shouldPickImmediately(_items) { return false }

  async mouseMove(vec, coords) {
    if(this.viewport.hoveredHandle || this.viewport.hoveredDimension) return this.viewport.renderer.render()
    if(this.viewport.pickingPath) this.viewport.updatePath(this.viewport.pickingPath)
    const object = await this.getObject(coords, true)
    this.viewport.$emit('update:highlight', object)
    return object
  }

  getObject(coords, any) {
    return new Promise(resolve => {
      let items = this.viewport.renderer
        .objectsAtScreen(coords, this.realSelectors, this.includeInactive, this.visibleEdgesOnly)
        .flatMap(obj => {
          if(!obj.alcTypes.some(type => type == 'face')) return obj.alcObject
          const mapped = []
          if(this.selectors.includes('solid')) mapped.push(obj.alcObject.solid)
          if(this.selectors.includes('component')) mapped.push(obj.alcObject.solid.component)
          if(mapped.length) return mapped
          return obj.alcObject
        })
      items = Array.from(new Set(items)).filter(item => !item.topology || item.topology.origin.feature)
      const handle = this.viewport.hoveredHandle
      if(handle && this.realSelectors.includes('point') && !this.preferPointObjects) {
        items.unshift(new ElemRef(handle.elem, handle.index))
      }
      const acceptsInput = this.acceptsInput?.bind(this) ||
        this.viewport.document.activeFeature?.acceptsInput?.bind(this.viewport.document.activeFeature)
      if(acceptsInput) items = items.filter(item => acceptsInput(item))
      items = this.orderItems(items)
      if(items.length > 1 && !any && !this.shouldPickImmediately(items)) {
        // Combat close-widgets event
        setTimeout(() => this.viewport.widgets.push({
          items,
          pos: coords,
          cb: (choice) => resolve(choice),
        }))
      } else {
        resolve(items[0])
      }
    })
  }
}


export class ManipulationTool extends HighlightTool {
  constructor(component, viewport) {
    super(component, viewport, [])
    this.localSpace = true
    this.preferPointObjects = true
    this.visibleEdgesOnly = !this.viewport.document.activeSketch
    this.probesSurface = this.visibleEdgesOnly
    this.setSelectors(this.viewport.document.activeSketch ? ['curve', 'point'] : ['curve', 'point', 'face', 'edge', 'plane'])
  }

  acceptsInput(item) {
    if(item instanceof PlaneHelper) {
      const defaultPlaneIds = ['XY', 'XZ', 'YZ'].map(name => `${item.componentId}/${name}`)
      return !defaultPlaneIds.includes(item.id)
    }
    return !(item instanceof PointHelper)
  }

  orderItems(items) {
    const closest = items[0]
    if(this.viewport.document.activeSketch || !(closest instanceof Face || closest instanceof Edge)) return items
    const edge = items.find(item => item instanceof Edge)
    return edge ? [edge, ...items.filter(item => item != edge)] : items
  }

  shouldPickImmediately(items) {
    return items.every(item => item instanceof Face || item instanceof Edge)
  }

  click(vec, coords) {
    if(!this.viewport.document.activeSketch) {
      this.selectionClickPending = true
      return
    }
    return this.selectAt(coords, this.viewport.bus.isCtrlPressed)
  }

  canvasClick(coords, event) {
    if(!this.selectionClickPending) return
    this.selectionClickPending = false
    this.doubleClickPending = event.detail == 2
    if(event.detail != 1) return
    // Native click counts distinguish a double click without delaying face
    // selection. Keep the original selection for Ctrl-double-click toggling.
    this.selectionBeforeClick = [...this.viewport.document.selection.items]
    return this.selectAt(coords, event.ctrlKey || event.metaKey)
  }

  async doubleClick(coords, event) {
    if(!this.doubleClickPending) return
    this.doubleClickPending = false
    const item = await this.getObject(coords, true)
    if(item instanceof SketchElement) {
      this.viewport.document.activateSketch(item.sketch)
      return
    }
    if(!(item instanceof Face || item instanceof Edge)) return
    const selection = this.viewport.document.selection
    const additive = event.ctrlKey || event.metaKey
    if(additive) {
      selection.clear()
      this.selectionBeforeClick.forEach(selected => selection.add(selected))
    }
    selection.handle(item.solid, additive)
  }

  async selectAt(coords, additive) {
    const item = await this.getObject(coords)
    if(item) {
      this.viewport.document.selection.handle(item, additive)
    } else {
      if(additive) return this.viewport.renderer.render()
      this.viewport.document.selection.clear()
    }
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    this.pointerDown = true
    this.selectionClickPending = false
    this.doubleClickPending = false
    const picked = await this.getObject(coords, true)
    const object = picked instanceof Face || picked instanceof Edge ? picked.solid : picked
    if(!this.pointerDown) return
    if(object instanceof Solid) {
      this.object = object
      this.startCoords = coords
      this.startWorld = worldTransform(this.object.component)
      const hit = this.viewport.renderer.hitTest(coords).find(intersection =>
        intersection.object.alcObject?.solid == object
      )
      const dragOrigin = hit?.point || new THREE.Vector3().setFromMatrixPosition(this.startWorld)
      const cameraDirection = this.viewport.renderer.activeCamera.getWorldDirection(new THREE.Vector3())
      this.dragPlane = new THREE.Plane().setFromNormalAndCoplanarPoint(cameraDirection, dragOrigin)
      this.dragStart = this.viewport.renderer.intersectScreenPlane(coords, this.dragPlane)
      this.dragPoint = hit
        ? hit.point.clone().applyMatrix4(this.startWorld.clone().invert())
        : new THREE.Vector3()
    } else if(
      !this.viewport.activeHandle &&
      !this.viewport.activeDimension &&
      !this.viewport.hoveredHandle &&
      !this.viewport.hoveredDimension &&
      (object instanceof Line || object instanceof Circle || object instanceof Arc) &&
      this.viewport.document.activeSketch?.elements.includes(object) &&
      !object.projection
    ) {
      const selectedElements = this.viewport.document.selection.items.filter(item =>
        this.viewport.document.activeSketch.elements.includes(item) && !(item instanceof OffsetCurve)
      )
      const group = selectedElements.includes(object) && selectedElements.length > 1 ?
        selectedElements.map(elem => ({
          elem,
          handles: elem.handles().map(handle => handle.clone()),
        }))
        : null
      this.curveDrag = {
        pointer: vec.clone(),
        coords: coords.clone(),
        handles: object.handles().map(handle => handle.clone()),
        group,
      }
      // Keep the pointer gesture separate from the sketch. mouseMove supplies
      // point or radius objectives and the solver commits accepted geometry.
      this.viewport.activeHandle = {
        type: 'curve',
        elem: object,
        index: 0,
        curveDrag: true,
      }
    } else if(
      !object &&
      this.viewport.document.activeSketch &&
      !this.viewport.activeHandle &&
      !this.viewport.activeDimension &&
      !this.viewport.hoveredHandle &&
      !this.viewport.hoveredDimension
    ) {
      this.boxDrag = { start: coords.clone() }
    }
    if(!this.viewport.activeHandle && !this.viewport.activeDimension) return
    // A curve body has no single snap handle under the pointer. Snapping it
    // as though its first handle were being dragged would create surprising
    // endpoint/center constraints on release.
    this.snapToPoints = !this.curveDrag && !this.viewport.activeDimension
    this.cursor = 'grabbing'
  }

  mouseUp(vec, coords) {
    this.pointerDown = false
    const boxDrag = this.boxDrag
    const boxSelecting = boxDrag && coords.distanceTo(boxDrag.start) > dragThreshold
    super.mouseUp(vec, coords)
    this.snapToPoints = false
    this.cursor = 'auto'
    delete this.object
    delete this.dragPoint
    delete this.dragPlane
    delete this.dragStart
    const sketch = this.viewport.document.activeSketch
    const handle = this.viewport.activeHandle
    if(sketch && handle && !handle.curveDrag) {
      const snap = captureSnap(this.viewport.snapper)
      const position = handle.elem.handles()[handle.index]
      // A blocked pointer target must not create a constraint at a position
      // the geometry never reached. Drag release never dissolves points.
      if(snap?.point && position.distanceTo(snap.point) <= EPSILON) {
        const constraints = [...sketch.constraints]
        const elements = [...sketch.elements]
        addCapturedSnapConstraint(sketch, snap, handle.elem, handle.index, true)
        if(sketch.solve(this.viewport.document.top(), undefined, { applyResults: false }) === false) {
          sketch.constraints = constraints
          sketch.elements.filter(elem => !elements.includes(elem)).forEach(elem => elem.clear())
          sketch.elements = elements
        }
      }
    }
    if(boxSelecting) this.selectBox(boxDrag.start, coords)
    this.viewport.selectionBox = null
    delete this.boxDrag
    delete this.curveDrag
  }

  cancelPointer() {
    super.cancelPointer()
    this.pointerDown = false
    this.selectionClickPending = false
    this.doubleClickPending = false
    this.snapToPoints = false
    this.cursor = 'auto'
    delete this.object
    delete this.dragPoint
    delete this.dragPlane
    delete this.dragStart
    delete this.curveDrag
    delete this.boxDrag
    this.viewport.selectionBox = null
  }

  dispose() { this.cancelPointer() }

  mouseMove(vec, coords) {
    const handle = this.viewport.activeHandle
    const dimension = this.viewport.activeDimension

    // Drag a line by its body, or change a circle/arc radius from the curve.
    // Updates are absolute from mouse-down, so solver adjustments between
    // pointer events cannot build up into drift.
    if(handle?.curveDrag && this.curveDrag) {
      if(coords.distanceTo(this.curveDrag.coords) <= dragThreshold) return
      const originalHandles = this.curveDrag.handles
      if(this.curveDrag.group) {
        const delta = vec.clone().sub(this.curveDrag.pointer)
        handle.targets = this.curveDrag.group.flatMap(item => item.handles.map((point, index) => ({
          elem: item.elem, index, point: point.clone().add(delta),
        })))
      } else if(handle.elem instanceof Line) {
        const delta = vec.clone().sub(this.curveDrag.pointer)
        handle.targets = originalHandles.map((point, index) => ({
          elem: handle.elem, index, point: point.clone().add(delta),
        }))
      } else {
        const center = originalHandles[0].clone()
        const radius = center.distanceTo(vec)
        if(radius <= EPSILON) return
        handle.targets = [
          { elem: handle.elem, index: 0, point: center },
          { elem: handle.elem, radius },
        ]
      }
      this.viewport.updateSketch(true)

    // Drag Handles
    } else if(handle) {
      handle.targets = [{ elem: handle.elem, index: handle.index, point: vec.clone() }]
      this.viewport.updateSketch(true)

    // Drag Dimensions
    } else if(dimension) {
      dimension.position = vec
      this.viewport.updateSketch()

    // Drag Solids
    } else if(this.object) {
      if(coords.distanceTo(this.startCoords) <= dragThreshold) return
      const dragEnd = this.viewport.renderer.intersectScreenPlane(coords, this.dragPlane)
      if(!dragEnd || !this.dragStart) return
      const translation = dragEnd.sub(this.dragStart)
      const desiredWorld = new THREE.Matrix4().makeTranslation(translation).multiply(this.startWorld)

      const comp = this.object.component
      const tree = this.viewport.document.top()
      // The target is absolute relative to mouse-down, but the solve should be
      // continuous. Keeping the previous solution as the next initial pose
      // prevents multi-joint chains from jumping to another valid branch.
      solveAssembly(tree, comp, desiredWorld, 64, this.dragPoint)

    // Drag Empty Sketch Area
    } else if(this.boxDrag) {
      if(coords.distanceTo(this.boxDrag.start) <= dragThreshold) return
      this.viewport.selectionBox = this.screenRect(this.boxDrag.start, coords)
      this.viewport.$emit('update:highlight', null)

    } else {
      super.mouseMove(vec, coords)
    }
  }

  screenRect(start, end) {
    return {
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      width: Math.abs(end.x - start.x),
      height: Math.abs(end.y - start.y),
    }
  }

  selectBox(start, end) {
    const rect = this.screenRect(start, end)
    const sketch = this.viewport.document.activeSketch
    const transform = this.viewport.snapper.planeTransform
    const contains = point => {
      const screen = this.viewport.renderer.toScreen(point.clone().applyMatrix4(transform))
      return screen.x >= rect.x && screen.x <= rect.x + rect.width &&
        screen.y >= rect.y && screen.y <= rect.y + rect.height
    }
    const selected = sketch.elements.filter(elem => {
      const vertices = elem instanceof SketchPoint ? elem.handles() : elem.tesselate?.()
      const points = (vertices || elem.handles?.() || []).map(vertex =>
        vertex instanceof THREE.Vector3 ? vertex : new THREE.Vector3(...vertex)
      )
      return points.length > 0 && points.every(contains)
    })

    const selection = this.viewport.document.selection
    if(!this.viewport.bus.isCtrlPressed) selection.clear()
    selected.forEach(item => this.viewport.bus.isCtrlPressed ? selection.toggle(item) : selection.add(item))
    this.viewport.renderer.render()
  }
}


// Adds reference geometry derived from items outside the active sketch
export class ProjectionTool extends HighlightTool {
  constructor(component, viewport, sketch, selectors) {
    super(component, viewport, selectors)
    this.sketch = sketch
    this.localSpace = true
    this.includeInactive = true
    this.cursor = 'copy'
  }

  fromOtherSketch(item) {
    const sketch = item instanceof ElemRef ? item.curve().sketch : item.sketch
    return toRaw(sketch) != toRaw(this.sketch)
  }

  async mouseMove(vec, coords) {
    const object = await this.getObject(coords, true)
    this.viewport.$emit('update:highlight', object)
  }

  async click(vec, coords) {
    const item = await this.getObject(coords)
    if(!item) return
    const projection = this.createProjection(item)
    projection.sketch = this.sketch
    if(!projection.update(this.viewport.document.top())) {
      this.viewport.bus.emit('toast', projection.error)
      return
    }
    this.sketch.addProjection(projection)
    this.viewport.updateRegions(true)
  }
}


export class ProjectTool extends ProjectionTool {
  static icon = 'sketch-project'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch, ['point', 'edge', 'curve'])
  }

  acceptsInput(item) {
    return item instanceof Edge || item instanceof Vertex || item instanceof PointHelper ||
      ((item instanceof SketchElement || item instanceof ElemRef) && this.fromOtherSketch(item))
  }

  createProjection(item) { return new Projection(item) }
}


export class IntersectTool extends ProjectionTool {
  static icon = 'sketch-intersect'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch, ['face', 'edge', 'curve'])
  }

  acceptsInput(item) {
    return item instanceof Face || item instanceof Edge ||
      (item instanceof SketchElement && !(item instanceof SketchPoint) && this.fromOtherSketch(item))
  }

  createProjection(item) { return new Intersection(item) }
}


export class SketchFilletTool extends HighlightTool {
  static icon = 'sketch-fillet'
  static hasOptions = true

  constructor(component, viewport, sketch) {
    super(component, viewport, ['curve', 'point'])
    this.sketch = sketch
    this.localSpace = true
    this.cursor = 'crosshair'
    this.radius = 1
    this.items = []
    this.fillets = []
    this.settings = {
      radius: { title: 'Radius', type: 'number', min: EPSILON * 2, step: 1, focus: true },
    }
  }

  showsHandles(elem) { return elem != this.arc }

  shouldPickImmediately() { return true }

  acceptsInput(item) {
    const curve = item instanceof ElemRef ? item.curve() : item
    // The hovered handle can still belong to a curve the last fillet replaced.
    return supportsFillet(curve) && curve != this.arc && this.sketch.elements.includes(curve) && !curve.projection
  }

  cornerCurves(item) {
    if(!(item instanceof ElemRef)) return this.items.length ? [this.items[0], item] : null
    const curve = item.curve()
    const point = curve.handles()[item.index]
    if(!curve.endpoints().some(end => end.almost(point))) return null
    const neighbors = this.sketch.elements.filter(other => supportsFillet(other) && other != curve && other != this.arc &&
      !other.projection && other.isReference == curve.isReference && other.endpoints().some(end => end.almost(point)))
    return neighbors.length == 1 ? [curve, neighbors[0]] : null
  }

  setOption(key, value) {
    if(key != 'radius') return
    this.radius = value
    if(this.previewCurves) this.updatePreview(this.previewCurves)
  }

  async mouseMove(vec, coords) {
    const item = await this.getObject(coords, true)
    if(this.disposed) return
    this.viewport.$emit('update:highlight', item)
    const curves = item && this.cornerCurves(item)
    if(curves) this.updatePreview(curves)
    else this.clearPreview()
  }

  async click(vec, coords) {
    const item = await this.getObject(coords)
    if(!item || this.disposed) return
    const curves = this.cornerCurves(item)
    if(!curves) {
      this.clearPreview()
      this.items = [item instanceof ElemRef ? item.curve() : item]
      this.viewport.renderer.render()
      return
    }
    if(!this.updatePreview(curves)) {
      this.viewport.bus.emit('toast', this.previewError)
      return
    }
    const arc = this.arc
    const { ends, parameters, points } = this.fillet
    this.fillets.push({ arc, position: this.dimensionPosition() })
    this.arc = null
    curves.forEach((curve, i) => {
      const { curve: replacement, index } = trimToPoint(curve, ends[i], parameters[i], points[i])
      replacement.isReference = curve.isReference
      this.sketch.replaceElement(curve, [replacement])
      this.sketch.addConstraint(new CoincidentConstraint(new ElemRef(replacement, index), new ElemRef(arc, i + 1)))
      this.sketch.addConstraint(new TangentConstraint(
        replacement instanceof Spline ? new ElemRef(replacement, index) : replacement, arc))
      this.viewport.document.selection.delete(curve)
    })
    this.items = []
    this.previewCurves = null
    this.viewport.$emit('update:highlight', null)
    this.viewport.updateRegions(true)
    this.viewport.renderer.render()
  }

  updatePreview(curves) {
    if(this.arc && this.previewRadius == this.radius &&
      this.previewCurves.every((curve, i) => curve == curves[i])) return true
    this.clearPreview()
    this.previewCurves = curves
    try {
      if(curves[0] == curves[1]) throw new Error('Select a different curve for the fillet')
      if(curves[0].isReference != curves[1].isReference) throw new Error('Both curves must have the same construction status')
      this.fillet = curveFillet(...curves, this.radius)
      this.arc = new Arc(this.fillet.center, this.radius, this.fillet.start, this.fillet.sweep)
      this.arc.isReference = curves[0].isReference
      this.sketch.add(this.arc)
      this.previewRadius = this.radius
      this.previewError = null
      this.viewport.renderer.render()
      return true
    } catch(error) {
      this.previewError = error.message
      return false
    }
  }

  dimensionPosition() {
    const direction = this.fillet.middle.clone().sub(this.fillet.center).normalize()
    return this.fillet.middle.clone().addScaledVector(direction, 10)
  }

  clearPreview() {
    if(this.arc) this.arc.remove()
    this.arc = null
    this.previewCurves = null
    this.viewport.renderer.render()
  }

  dispose() {
    this.disposed = true
    this.clearPreview()
    this.items = []
    const fillets = this.fillets.filter(({ arc }) => this.sketch.elements.includes(arc))
    if(fillets.length) {
      const first = fillets[0]
      fillets.slice(1).forEach(({ arc }) => this.sketch.addConstraint(new EqualConstraint(first.arc, arc)))
      this.sketch.addConstraint(new Dimension([first.arc], first.position, this.radius))
      this.viewport.updateRegions(true)
    }
    this.fillets = []
  }
}


// Base for tools that replace a clicked sketch curve with edited pieces.
class CurveEditTool extends Tool {
  constructor(component, viewport, sketch) {
    super(component, viewport)
    this.sketch = sketch
    this.localSpace = true
    this.cursor = 'crosshair'
  }

  curveAt(coords) {
    const object = this.viewport.renderer.objectsAtScreen(coords, ['curve'])[0]
    const curve = object && object.alcObject
    return curve && curve.sketch == this.sketch && !curve.projection && !(curve instanceof OffsetCurve) ? curve : null
  }

  finish(curve) {
    this.viewport.document.selection.delete(curve)
    this.viewport.$emit('update:highlight', null)
    this.viewport.updateRegions(true)
    this.viewport.renderer.render()
  }

  mouseMove(vec, coords) {
    const curve = this.curveAt(coords)
    this.viewport.$emit('update:highlight', curve)
  }
}


// Base for tools editing a curve split at its intersections. Subclasses plan
// the edit for the hovered segment as { preview, cuts, replacements, joins }:
// the piece the click affects, the points the curve is cut at, the curves
// replacing it and optional [curve, index, curve, index] handle pairs to
// keep coincident. That piece and those points are previewed instead of
// highlighting the whole curve.
class SegmentEditTool extends CurveEditTool {
  // Split the hovered curve at all intersections, reusing the split and its
  // plans while the same curve stays hovered.
  planAt(coords, vec) {
    const curve = this.curveAt(coords)
    if(!curve) return {}
    if(this.split?.curve != curve) {
      this.split = { curve, pieces: curve.split(this.sketch.elements), plans: new Map() }
    }
    const pieces = this.split.pieces
    if(pieces.length < 2) return { curve }
    const segment = pieces.minMaxBy(Math.min, piece => piece.distanceTo(vec))
    return { curve, plan: this.plan(curve, pieces, segment, vec) }
  }

  // Plans are cached so the preview is only rebuilt when the edit changes.
  cachedPlan(key, build) {
    if(!this.split.plans.has(key)) this.split.plans.set(key, build())
    return this.split.plans.get(key)
  }

  mouseMove(vec, coords) {
    const { curve, plan } = this.planAt(coords, vec)
    this.viewport.$emit('update:highlight', null)
    if(plan == this.previewed) return
    this.clearPreview()
    if(plan) this.showPreview(curve, plan)
    this.viewport.renderer.render()
  }

  click(vec, coords) {
    const { curve, plan } = this.planAt(coords, vec)
    if(curve && !plan) window.bus.emit('toast', this.constructor.noSegmentMessage)
    if(!plan) return this.viewport.renderer.render()

    const { replacements } = plan
    replacements.forEach(piece => piece.isReference = curve.isReference)
    this.split.pieces.filter(piece => !replacements.includes(piece)).forEach(piece => piece.clear())
    this.sketch.replaceElement(curve, replacements)
    plan.joins?.forEach(([left, leftIndex, right, rightIndex]) => this.sketch.addConstraint(
      new CoincidentConstraint(new ElemRef(left, leftIndex), new ElemRef(right, rightIndex))
    ))
    this.split = null
    this.clearPreview()
    this.finish(curve)
  }

  showPreview(curve, plan) {
    const renderer = this.viewport.renderer
    this.previewed = plan
    this.preview = markRaw(new THREE.Group())
    this.preview.matrix.copy(curve.mesh().matrix)
    this.preview.matrix.decompose(this.preview.position, this.preview.quaternion, this.preview.scale)
    const line = renderer.convertLine(plan.preview.tesselate(), this.constructor.previewMaterial)
    renderer.offsetSketchObject(line)
    // Draw over the curve's own line, which lies at the same depth.
    line.renderOrder = 1
    this.preview.add(line)
    plan.cuts.forEach(point => {
      const dot = new ScreenPointObject(9, materials.highlightUiPoint)
      dot.position.copy(point)
      this.preview.add(dot)
    })
    renderer.add(this.preview)
  }

  clearPreview() {
    this.viewport.renderer.remove(this.preview)
    this.preview = null
    this.previewed = null
  }

  dispose() {
    this.clearPreview()
    this.viewport.renderer.render()
  }
}


export class TrimTool extends SegmentEditTool {
  static icon = 'sketch-trim'
  static noSegmentMessage = 'Curve has no intersection to trim to'
  static previewMaterial = materials.trimLine

  // The rest of a closed curve stays one arc. An open curve keeps one piece
  // before and one after the segment, rather than every split piece.
  plan(curve, pieces, segment) {
    return this.cachedPlan(segment, () => {
      if(curve.isClosed()) {
        return { preview: segment, cuts: segment.endpoints(), replacements: [circleRest(segment)] }
      }
      const index = pieces.indexOf(segment)
      const [start, end] = segment.endpoints()
      return {
        preview: segment,
        cuts: [index > 0 && start, index < pieces.length - 1 && end].filter(Boolean),
        replacements: [
          index > 0 && trimToPoint(curve, 1, curve.unsample(start), start.clone()),
          index < pieces.length - 1 && trimToPoint(curve, 0, curve.unsample(end), end.clone()),
        ].filter(Boolean).map(part => part.curve),
      }
    })
  }
}


// Split the clicked curve at the intersection nearest to the cursor. The
// preview shows the resulting piece on the cursor's side.
export class BreakTool extends SegmentEditTool {
  static icon = 'sketch-break'
  static noSegmentMessage = 'Curve has no intersection to break at'
  static previewMaterial = materials.highlightLine

  plan(curve, pieces, segment, vec) {
    // A closed curve needs two cuts, so the hovered segment is broken out.
    if(curve.isClosed()) return this.cachedPlan(segment, () => {
      const rest = circleRest(segment)
      return {
        preview: segment,
        cuts: segment.endpoints(),
        replacements: [segment, rest],
        joins: [[segment, 2, rest, 1], [rest, 2, segment, 1]],
      }
    })

    // Open curve pieces are in traversal order, so cut i joins piece i to
    // piece i + 1.
    const index = pieces.indexOf(segment)
    const cut = [index - 1, index].filter(i => i >= 0 && i < pieces.length - 1)
      .minMaxBy(Math.min, i => pieces[i].endpoints()[1].distanceTo(vec))
    return this.cachedPlan(cut, () => {
      const point = pieces[cut].endpoints()[1]
      const parameter = curve.unsample(point)
      // Cut pieces get a handle at the cut, so they can be joined there.
      const [first, second] = [1, 0].map(end => trimToPoint(curve, end, parameter, point.clone()))
      return {
        preview: (index > cut ? second : first).curve,
        cuts: [point],
        replacements: [first.curve, second.curve],
        joins: [[first.curve, first.index, second.curve, second.index]],
      }
    })
  }
}

// The arc completing a circle from the end of one of its arc pieces back to
// that piece's start.
function circleRest(segment) {
  const direction = Math.sign(segment.sweep)
  return new Arc(segment.center(), segment.radius,
    segment.start + segment.sweep, direction * (Math.PI * 2 - Math.abs(segment.sweep)))
}


// Lengthen the end nearest to the click up to the closest curve it meets.
export class ExtendTool extends CurveEditTool {
  static icon = 'sketch-extend'

  click(vec, coords) {
    const curve = this.curveAt(coords)
    if(!curve) return this.viewport.renderer.render()
    if(!(curve instanceof Line || curve instanceof Arc)) {
      window.bus.emit('toast', 'Only lines and arcs can be extended')
      return this.viewport.renderer.render()
    }

    const endpoints = curve.endpoints()
    const end = endpoints[0].distanceTo(vec) < endpoints[1].distanceTo(vec) ? 0 : 1
    const point = endpoints[end]
    const targets = [
      ...this.sketch.elements,
      ...this.sketch.projectedElements(),
    ].filter(elem => elem != curve && elem.geom)
    const extension = curve instanceof Line ? lineExtension(curve, end, targets) : arcExtension(curve, end)
    const start = extension.range().start
    const hits = targets.flatMap(target => extension.intersections(target.geom(), modelTolerance)
      .filter(hit => hit.kind == 'point')
      .map(hit => ({
        target,
        point: new THREE.Vector3(hit.point.x, hit.point.y, 0),
        distance: Math.abs(hit.firstParameter - start),
      }))
      .filter(hit => hit.point.distanceTo(point) > EPSILON))
    if(!hits.length) {
      window.bus.emit('toast', 'No curve to extend to')
      return this.viewport.renderer.render()
    }

    const hit = hits.minMaxBy(Math.min, hit => hit.distance)
    const replacement = extendedCurve(curve, end, hit)
    replacement.isReference = curve.isReference
    this.sketch.replaceElement(curve, [replacement])
    const targetIndex = hit.target.handles().findIndex(handle => handle.almost(hit.point))
    const snap = isConstrainablePoint(hit.target, targetIndex) ?
      { elem: hit.target, index: targetIndex } : { curve: hit.target }
    addCapturedSnapConstraint(this.sketch, snap, replacement, curve instanceof Line ? end : end + 1, true)
    this.finish(curve)
  }
}

// A segment from the line's end, long enough to reach every target.
function lineExtension(line, end, targets) {
  const point = line.points[end]
  const direction = point.clone().sub(line.points[1 - end]).normalize()
  const reach = Math.max(0, ...targets.flatMap(target =>
    target.tesselate().map(([x, y]) => point.distanceTo(new THREE.Vector3(x, y, 0)))))
  return sx.TrimmedCurve2.line(point2(point), point2(point.clone().addScaledVector(direction, reach + 1)))
}

// The rest of the arc's circle, traversed away from the given end.
function arcExtension(arc, end) {
  const geom = arc.geom(), range = geom.range()
  const direction = Math.sign(arc.sweep) * (end ? 1 : -1)
  const from = end ? range.end : range.start
  return sx.TrimmedCurve2.new(geom.curve(),
    sx.ParameterRange.new(from, from + direction * (Math.PI * 2 - Math.abs(arc.sweep))))
}

function extendedCurve(curve, end, hit) {
  if(curve instanceof Line) {
    const points = curve.points.map(point => point.clone())
    points[end] = hit.point
    return new Line(...points)
  }
  const growth = Math.sign(curve.sweep) * hit.distance
  return new Arc(curve.center(), curve.radius, end ? curve.start : curve.start - growth, curve.sweep + growth)
}


class PickTool extends HighlightTool {
  constructor(component, viewport, selectors, callback) {
    super(component, viewport, selectors)
    this.callback = callback
    this.localSpace = false
    this.probesSurface = true
    this.chooseAmongItems = false
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const object = await this.getObject(coords, !this.chooseAmongItems)
    if(!object || this.disposed) return
    this.callback(object)
  }

  dispose() { this.disposed = true }
}


export class CurvePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['curve'], callback)
  }
}

export class PathPickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['path'], callback)
  }
}

export class ProfilePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['profile'], callback)
  }
}

export class FacePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['face'], callback)
  }
}

export class EdgePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['edge'], callback)
  }
}

export class AxisPickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['axis'], callback)
  }
}

export class PlanePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['plane'], callback)
    // Planar faces on inactive components are ghosted and live outside the
    // normal raycast tree, but they can still support a new sketch.
    this.includeInactive = true
  }

  // Construction planes should remain directly pickable when planar model
  // faces cover them. Preserve raycast order within both candidate groups.
  orderItems(items) {
    return items.filter(item => item instanceof PlaneHelper).concat(
      items.filter(item => !(item instanceof PlaneHelper))
    )
  }
}

export class SolidPickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['solid'], callback)
    // Feature-level input predicates still decide whether an inactive solid is
    // valid. Boolean features use this to accept tools from other components.
    this.includeInactive = true
  }
}

export class PointPickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['point'], callback)
  }
}

export class ComponentPickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['component'], callback)
  }
}

export class JointPickTool extends PickTool {
  constructor(component, viewport, callback) {
    // Joint markers are projected DOM controls, so canvas raycasting is only
    // used to keep hover clearing and picker lifecycle consistent.
    super(component, viewport, ['joint'], callback)
  }

  pick(joint) { if(!this.disposed) this.callback(joint) }
}

export class PatternInputPickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['solid', 'component'], callback)
    this.chooseAmongItems = true
  }
}


export class SketchTool extends Tool {
  constructor(component, viewport, sketch) {
    super(component, viewport)
    this.sketch = sketch
    this.localSpace = true
    this.snapToGuides = true
    this.snapToPoints = true
    this.cursor = 'crosshair'
    this.locks = {}
    this.inputStage = 0
  }

  setLock(key, value) {
    if(value === undefined) delete this.locks[key]
    else this.locks[key] = value
    if(this.pointer) this.mouseMove(this.pointer)
  }

  hasLocks() { return Object.keys(this.locks).length > 0 }

  resetInput() {
    this.locks = {}
    this.inputStage++
  }

  // Locked values override the cursor position, so the snapped target
  // under the cursor no longer describes the placed point.
  pointerSnap() {
    return this.hasLocks() ? undefined : captureSnap(this.viewport.snapper)
  }

  submitInput() {
    if(!this.pointer) return
    this.mouseDown(this.pointer, null)
    this.viewport.updateRegions(false)
  }

  addLockedDimension(key, items, position) {
    if(this.locks[key] === undefined) return
    this.sketch.addConstraint(new Dimension(items, position))
  }

  constrainPoint(elem, index, snap) {
    if(arguments.length < 3) snap = captureSnap(this.viewport.snapper)
    const captured = addCapturedSnapConstraint(this.sketch, snap, elem, index)
    const origin = this.originSnapConstraint(elem, index, snap)
    if(snap?.tangent) this.sketch.addConstraint(new TangentConstraint(
      elem instanceof Spline ? new ElemRef(elem, index) : elem,
      snap.tangent,
    ))
    return captured || origin
  }

  originSnapConstraint(elem, index, snap) {
    const origin = this.sketch.origin()
    const originPoint = this.sketch.originPoint()
    if(!origin || !originPoint) return
    const { x, y } = snap || {}
    const snapX = x && x.almost(originPoint)
    const snapY = y && y.almost(originPoint)
    if(!snapX && !snapY) return
    const ref = new ElemRef(elem, index)
    const originRef = new ElemRef(origin)
    const constraint = snapX && snapY ?
      new CoincidentConstraint(ref, originRef)
      :
      new HorVertConstraint(ref, originRef)
    this.sketch.addConstraint(constraint)
    return constraint
  }

  snappedDirectlyToOrigin() {
    if(this.hasLocks()) return false
    const origin = this.sketch.originPoint()
    const { x, y } = this.viewport.snapper.snapped || {}
    return origin && x && y && x.almost(origin) && y.almost(origin)
  }
}


export class PasteSketchTool extends SketchTool {
  static icon = 'paste'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    Object.assign(this, pasteSketchSelection(sketch))
    this.handles = this.elements.map(elem => elem.setHandles ? elem.handles().map(point => point.clone()) : null)
    this.positions = this.constraints.map(constraint => constraint.position?.clone())
    this.snapToGuides = false
    this.snapToPoints = false
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    const delta = vec.clone().sub(this.anchor)
    this.elements.forEach((elem, index) => {
      if(this.handles[index]) elem.setHandles(this.handles[index].map(point => point.clone().add(delta)))
    })
    this.constraints.forEach((constraint, index) => {
      if(this.positions[index]) constraint.position = this.positions[index].clone().add(delta)
      if(constraint instanceof ProfileOffsetConstraint) constraint.rebuild()
    })
    this.viewport.elementChanged()
  }

  click(vec) {
    this.mouseMove(vec)
    this.constraints.forEach(constraint => this.sketch.addConstraint(constraint))
    this.viewport.document.selection.clear()
    this.elements.forEach(elem => this.viewport.document.selection.add(elem))
    this.elements = []
    this.viewport.activateTool(ManipulationTool)
    this.viewport.updateRegions(true)
  }

  dispose() {
    this.elements.forEach(elem => elem.remove())
    this.elements = []
    this.viewport.elementChanged()
  }
}

export class LineTool extends SketchTool {
  static icon = 'sketch-line'

  inputFields() {
    if(!this.curve) return []
    const start = this.curve.endpoints()[0]
    return polarFields(start, polarPoint(start, this.pointer || start, this.locks))
  }

  constrained(vec) {
    return this.curve ? polarPoint(this.curve.endpoints()[0], vec, this.locks) : vec
  }

  constrainCompletedLine(line, endSnap) {
    this.addLockedDimension('length', [line], dimensionPosition(line, line.length() * 0.2))
    this.constrainPoint(line, 1, endSnap)
    if(endSnap?.lineRelation) {
      const { elem, perpendicular } = endSnap.lineRelation
      const type = perpendicular ? PerpendicularConstraint : ParallelConstraint
      this.sketch.addConstraint(new type(line, elem))
      if(!(endSnap.curve || endSnap.elem || endSnap.intersection)) {
        addCapturedGuideConstraints(this.sketch, endSnap, line, 1)
      }
      return
    }
    if(!endSnap?.tangent) addInferredTangentConstraint(this.sketch, line, endSnap)
    addInferredTangentConstraint(this.sketch, line, this.startSnap, 0)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    vec = this.constrained(vec)
    this.mouseMove(vec)
    const snap = this.pointerSnap()

    const old = this.curve
    const elems = [...this.sketch.elements]
    if(this.curve) elems.pop()
    const touchesExisting = elems.find(elem => elem.endpoints().some(p => p.equals(vec) ) )

    // Restart the tool when the segment ends on existing geometry.
    if((touchesExisting || this.snappedDirectlyToOrigin() || (snap && (snap.curve || snap.elem || snap.intersection))) && this.curve) {
      this.sketch.add(this.curve)
      this.constrainCompletedLine(this.curve, snap)
      this.curve = null
      this.startSnap = null
      this.previousSegment = null
    } else {
      this.curve = new Line(vec, vec)
      this.sketch.add(this.curve)
      if(old) {
        this.sketch.addConstraint(
          new CoincidentConstraint(new ElemRef(this.curve, 0), new ElemRef(old, 1))
        )
      } else {
        this.constrainPoint(this.curve, 0, snap)
      }
      if(old) {
        this.constrainCompletedLine(old, snap)
        const ySnapped = old.endpoints()[0].x.almost(vec.x)
        const xSnapped = old.endpoints()[0].y.almost(vec.y)
        if(!snap?.lineRelation && (xSnapped || ySnapped)) {
          this.sketch.addConstraint(new HorVertConstraint(old))
        }
      }
      this.startSnap = snap
      this.previousSegment = old
    }
    this.resetInput()
  }

  mouseMove(vec) {
    this.pointer = vec
    if(!this.curve) return
    let p1 = this.curve.endpoints()[0]
    this.curve.setHandles([p1, this.constrained(vec)], false)
    this.viewport.elementChanged()
  }

  guideSnapPoints() {
    return this.curve ? [this.curve.endpoints()[0]] : []
  }

  lineSnapContext() {
    if(!this.curve) return null
    const snap = this.startSnap
    return {
      start: this.curve.endpoints()[0],
      perpendicularCurves: [this.previousSegment, snap?.curve, snap?.elem, ...(snap?.intersection || [])]
        .filter(elem => elem instanceof Line || elem instanceof Circle || elem instanceof Arc),
    }
  }

  tangentSnapStart() {
    return this.curve?.endpoints()[0]
  }

  dispose() {
    if(!this.curve) return
    this.curve.remove()
  }
}


export class PointTool extends SketchTool {
  static icon = 'sketch-point'

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)
    if(snap && snap.elem instanceof SketchPoint) return

    if(snap && snap.intersection) {
      this.sketch.findOrCreateIntersectionPoint(snap.intersection, snap.point, new SketchPoint(vec))
    } else {
      const point = new SketchPoint(vec)
      this.sketch.add(point)
      this.constrainPoint(point, 0, snap)
    }
    this.viewport.elementChanged()
  }
}


export class SplineTool extends SketchTool {
  static icon = 'sketch-spline'
  static hasOptions = true

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    this.mode = 'draw'
    this.settings = {
      mode: {
        title: 'Mode',
        type: 'select',
        options: {
          draw: { title: 'Draw spline', icon: 'pen' },
          insert: { title: 'Insert point', icon: 'plus-circle' },
          remove: { title: 'Remove point', icon: 'minus-circle' },
        },
      },
    }
  }

  setOption(key, value) {
    if(key != 'mode' || !this.settings.mode.options[value] || value == this.mode) return
    if(this.finishCurve()) this.viewport.updateRegions(true)
    this.mode = value
    this.snapToGuides = value == 'draw'
    this.snapToPoints = value != 'remove'
    this.viewport.$emit('update:highlight', null)
    this.viewport.snapper.reset()
  }

  curveAt(coords) {
    const object = this.viewport.renderer.objectsAtScreen(coords, ['curve'])[0]
    const curve = object && object.alcObject
    return curve instanceof Spline && curve.sketch == this.sketch && !curve.projection ? curve : null
  }

  splineAt(coords, snap = captureSnap(this.viewport.snapper)) {
    const snapped = [snap?.curve, snap?.elem, ...(snap?.intersection || [])].find(curve =>
      curve instanceof Spline && curve.sketch == this.sketch && !curve.projection
    )
    return snapped || this.curveAt(coords)
  }

  acceptsSnapElement(elem) {
    return this.mode != 'insert' ||
      (elem instanceof Spline && elem.sketch == this.sketch && !elem.projection)
  }

  tangentSnapStart() {
    return this.mode == 'draw' && this.curve ? this.curve.handles().at(-2) : null
  }

  lineSnapContext() {
    if(this.mode != 'draw' || !this.curve) return null
    return {
      start: this.curve.handles().at(-2),
      perpendicularCurves: [],
    }
  }

  constrainPoint(elem, index, snap) {
    if(arguments.length < 3) snap = captureSnap(this.viewport.snapper)
    const constraint = super.constrainPoint(elem, index, snap)
    if(snap?.lineRelation?.perpendicular) {
      const target = snap.lineRelation
      this.sketch.addConstraint(new PerpendicularConstraint(
        new ElemRef(elem, index),
        target.elem,
      ))
    }
    return constraint
  }

  shiftConstraintIndices(spline, fromIndex, offset) {
    this.sketch.constraints.forEach(constraint => constraint.items.forEach(item => {
      if(item.curve() == spline && item.index !== undefined && item.index >= fromIndex) {
        item.index += offset
      }
    }))
  }

  insertPoint(spline, vec) {
    const points = spline.handles()
    let segmentIndex = 0
    let closestDistance = Infinity
    points.slice(0, -1).forEach((start, index) => {
      const segment = points[index + 1].clone().sub(start)
      const lengthSquared = segment.lengthSq()
      const position = lengthSquared > EPSILON
        ? THREE.MathUtils.clamp(vec.clone().sub(start).dot(segment) / lengthSquared, 0.0, 1.0)
        : 0.0
      const distance = start.clone().addScaledVector(segment, position).distanceToSquared(vec)
      if(distance >= closestDistance) return
      closestDistance = distance
      segmentIndex = index
    })

    const index = segmentIndex + 1
    const point = points[segmentIndex].clone().lerp(points[index], 0.5)

    this.shiftConstraintIndices(spline, index, 1)
    points.splice(index, 0, point)
    spline.setControlPoints(points)
    this.sketch.elements.forEach(element => element.fullyConstrained = false)
    this.viewport.elementChanged()
  }

  removePoint(handle) {
    const spline = handle && handle.elem
    if(!(spline instanceof Spline) || spline.sketch != this.sketch || spline.projection) return

    const points = spline.handles()
    if(!Number.isInteger(handle.index) || handle.index < 0 || handle.index >= points.length) return
    if(points.length <= 2) {
      window.bus.emit('toast', 'Splines need at least two points')
      this.viewport.activeHandle = null
      this.viewport.hoveredHandle = null
      return
    }

    const removedConstraints = new Set(this.sketch.constraints.filter(constraint =>
      constraint.items.some(item => item.curve() == spline && item.index == handle.index)
    ))
    this.sketch.constraints = this.sketch.constraints.filter(constraint =>
      !removedConstraints.has(constraint)
    )
    this.shiftConstraintIndices(spline, handle.index + 1, -1)
    points.splice(handle.index, 1)
    spline.setControlPoints(points)
    this.sketch.elements.forEach(element => element.fullyConstrained = false)
    this.viewport.activeHandle = null
    this.viewport.hoveredHandle = null
    this.viewport.elementChanged()
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)
    if(this.mode == 'insert') {
      const spline = this.splineAt(coords, snap)
      if(spline) this.insertPoint(spline, vec)
      return
    }
    if(this.mode == 'remove') {
      this.removePoint(this.viewport.activeHandle)
      return
    }
    if(this.curve) {
      let points = this.curve.handles()
      points[points.length - 1] = vec//.toArray()
      const completesSpline = this.snappedDirectlyToOrigin() ||
        (snap && (snap.curve || snap.elem || snap.intersection))
      if(completesSpline) {
        this.curve.setHandles(points)
        this.constrainPoint(this.curve, points.length - 1, snap)
        this.curve = null
        this.endSnap = null
        this.viewport.elementChanged()
        return
      }
      // points.push(vec.toArray())
      points.push(vec)
      this.curve.setHandles(points)
      this.endSnap = snap
    } else {
      this.curve = new Spline([vec, vec])
      this.sketch.add(this.curve)
      this.constrainPoint(this.curve, 0, snap)
      // this.curve = this.sketch.add_spline([vec.toArray(), vec.toArray()])
      // this.curve.sketch = this.sketch
    }
    this.viewport.elementChanged()
  }

  mouseMove(vec, coords) {
    if(this.mode == 'insert') {
      this.viewport.$emit('update:highlight', this.splineAt(coords))
      return
    }
    if(this.mode == 'remove') {
      const handle = this.viewport.hoveredHandle
      const spline = handle?.elem instanceof Spline && handle.elem.sketch == this.sketch
        ? handle.elem
        : null
      this.viewport.$emit('update:highlight', spline)
      return
    }
    if(!this.curve) return
    let points = this.curve.handles()
    points[points.length - 1] = vec//.toArray()
    this.curve.setHandles(points)
    this.viewport.elementChanged()
  }

  finishCurve() {
    if(!this.curve) return false
    let points = this.curve.handles()
    points.pop()
    if(points.length < 2) {
      this.curve.remove()
      this.curve = null
      this.endSnap = null
      this.viewport.elementChanged()
      return true
    }
    this.curve.setHandles(points)
    this.constrainPoint(this.curve, points.length - 1, this.endSnap)
    this.curve = null
    this.endSnap = null
    this.viewport.elementChanged()
    return true
  }

  dispose() {
    if(this.finishCurve()) this.viewport.updateRegions(true)
    this.viewport.$emit('update:highlight', null)
  }
}


export class CircleTool extends SketchTool {
  static icon = 'sketch-circle'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  inputFields() {
    if(!this.center) return []
    const point = this.constrained(this.pointer || this.center)
    return [{ key: 'diameter', label: '⌀', dimension: 'length', value: point.distanceTo(this.center) * 2.0 }]
  }

  constrained(vec) {
    if(!this.center || this.locks.diameter === undefined) return vec
    return radialPoint(this.center, vec, this.locks.diameter / 2.0)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.center) {
      this.mouseMove(vec)
      if(!this.curve.geom) return
      const radiusPoint = this.constrained(vec)
      this.addLockedDimension('diameter', [this.curve],
        this.center.clone().lerp(radiusPoint, 1.5))
      this.center = null
      this.curve = null
    } else {
      this.center = vec
      this.curve = new Circle(vec, 0)
      this.sketch.add(this.curve)
      this.constrainPoint(this.curve, 0)
    }
    this.resetInput()
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    this.pointer = vec
    if(!this.center) return
    this.curve.setHandles([this.center, this.constrained(vec)])
    this.viewport.elementChanged()
  }

  dispose() {
    if(this.curve) this.curve.remove()
    this.curve = null
    this.center = null
  }
}


export class RectangleTool extends SketchTool {
  static icon = 'sketch-rectangle'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    this.lines = []
  }

  snapExclusions() { return this.lines }

  inputFields() {
    if(!this.corner) return []
    const point = this.constrained(this.pointer || this.corner)
    return [
      { key: 'width', label: 'W', dimension: 'length', value: Math.abs(point.x - this.corner.x) },
      { key: 'height', label: 'H', dimension: 'length', value: Math.abs(point.y - this.corner.y) },
    ]
  }

  constrained(vec) {
    if(!this.corner) return vec
    const point = vec.clone()
    const { width, height } = this.locks
    if(width !== undefined) point.x = this.corner.x + (Math.sign(vec.x - this.corner.x) || 1) * width
    if(height !== undefined) point.y = this.corner.y + (Math.sign(vec.y - this.corner.y) || 1) * height
    return point
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    vec = this.constrained(vec)
    const snap = this.pointerSnap()

    if(!this.corner) {
      this.corner = vec.clone()
      this.startSnap = snap
      this.lines.push(
        new Line(this.corner.clone(), this.corner.clone()),
        new Line(this.corner.clone(), this.corner.clone()),
        new Line(this.corner.clone(), this.corner.clone()),
        new Line(this.corner.clone(), this.corner.clone()),
      )
      this.lines.forEach(line => this.sketch.add(line))
    } else {
      this.mouseMove(vec)
      this.commit(snap)
      this.corner = null
      this.startSnap = null
      this.lines.length = 0
    }
    this.resetInput()
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    this.pointer = vec
    if(!this.corner) return

    vec = this.constrained(vec)
    const [x, y] = [vec.x, vec.y]
    const [cornerX, cornerY] = [this.corner.x, this.corner.y]
    const points = [
      this.corner,
      new THREE.Vector3(x, cornerY, vec.z),
      vec,
      new THREE.Vector3(cornerX, y, vec.z),
    ]
    this.lines.forEach((line, index) => {
      line.setHandles([points[index], points[(index + 1) % points.length]])
    })
    this.viewport.elementChanged()
  }

  commit(endSnap) {
    const [bottom, right, top, left] = this.lines

    // Keep the four corners topologically connected even though each side is
    // represented by an ordinary line element.
    ;[[bottom, 1, right, 0], [right, 1, top, 0],
      [top, 1, left, 0], [left, 1, bottom, 0]].forEach(([first, firstIndex, second, secondIndex]) => {
      this.sketch.addConstraint(new CoincidentConstraint(
        new ElemRef(first, firstIndex), new ElemRef(second, secondIndex)
      ))
      this.sketch.addConstraint(new PerpendicularConstraint(first, second))
    })
    this.sketch.addConstraint(new HorVertConstraint(bottom))

    const center = bottom.midpoint().lerp(top.midpoint(), 0.5)
    const offset = Math.max(bottom.length(), right.length()) * 0.15
    this.addLockedDimension('width', [bottom], dimensionPosition(bottom, offset, center))
    this.addLockedDimension('height', [right], dimensionPosition(right, offset, center))

    this.constrainPoint(bottom, 0, this.startSnap)
    this.constrainPoint(top, 0, endSnap)
  }

  dispose() {
    this.lines.forEach(line => line.remove())
    this.lines.length = 0
    this.corner = null
  }
}


export class SlotTool extends SketchTool {
  static icon = 'sketch-slot'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    this.elements = []
  }

  snapExclusions() { return this.elements }

  guideSnapPoints() {
    return this.firstCenter && !this.secondCenter ? [this.firstCenter] : []
  }

  inputFields() {
    if(!this.firstCenter) return []
    const point = this.constrained(this.pointer || this.firstCenter)
    if(!this.secondCenter) return polarFields(this.firstCenter, point)
    return [{ key: 'radius', label: 'R', dimension: 'length', value: this.radiusAt(point) }]
  }

  constrained(vec) {
    if(!this.firstCenter) return vec
    if(!this.secondCenter) return polarPoint(this.firstCenter, vec, this.locks)
    if(this.locks.radius === undefined) return vec
    const normal = this.slotNormal()
    if(normal.dot(vec.clone().sub(this.firstCenter)) < 0) normal.negate()
    return this.firstCenter.clone().addScaledVector(normal, this.locks.radius)
  }

  slotNormal() {
    const axis = this.secondCenter.clone().sub(this.firstCenter).normalize()
    return new THREE.Vector3(-axis.y, axis.x, 0)
  }

  radiusAt(sizePoint) {
    return Math.abs(sizePoint.clone().sub(this.firstCenter).dot(this.slotNormal()))
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    vec = this.constrained(vec)
    const snap = this.pointerSnap()

    if(!this.firstCenter) {
      this.firstCenter = vec.clone()
      this.firstCenterSnap = snap
    } else if(!this.secondCenter) {
      if(vec.distanceToSquared(this.firstCenter) < 1e-12) return
      this.secondCenter = vec.clone()
      this.secondCenterSnap = snap
      this.lockedLength = this.locks.length
      this.updateCenterline(this.secondCenter)
    } else {
      if(!this.updateShape(vec)) return
      this.commit()
      this.firstCenter = null
      this.secondCenter = null
      this.firstCenterSnap = null
      this.secondCenterSnap = null
      this.lockedLength = undefined
      this.centerline = null
      this.elements = []
    }
    this.resetInput()
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    this.pointer = vec
    if(!this.firstCenter) return
    if(!this.secondCenter) {
      if(this.updateCenterline(this.constrained(vec))) this.viewport.elementChanged()
      return
    }
    if(this.updateShape(this.constrained(vec))) this.viewport.elementChanged()
  }

  updateCenterline(end) {
    if(end.distanceToSquared(this.firstCenter) < 1e-12) return false
    if(this.centerline) {
      this.centerline.setHandles([this.firstCenter.clone(), end.clone()])
    } else {
      this.centerline = new Line(this.firstCenter.clone(), end.clone())
      this.centerline.isReference = true
      this.sketch.add(this.centerline)
      this.elements = [this.centerline]
    }
    return true
  }

  slotPoints(sizePoint) {
    const axis = this.secondCenter.clone().sub(this.firstCenter)
    if(axis.lengthSq() < 1e-12) return
    axis.normalize()
    const normal = new THREE.Vector3(-axis.y, axis.x, 0)
    const radius = Math.abs(sizePoint.clone().sub(this.firstCenter).dot(normal))
    if(radius < 1e-6) return
    const offset = normal.multiplyScalar(radius)
    return {
      topFirst: this.firstCenter.clone().add(offset),
      topSecond: this.secondCenter.clone().add(offset),
      bottomFirst: this.firstCenter.clone().sub(offset),
      bottomSecond: this.secondCenter.clone().sub(offset),
      outerFirst: this.firstCenter.clone().sub(axis.clone().multiplyScalar(radius)),
      outerSecond: this.secondCenter.clone().add(axis.multiplyScalar(radius)),
    }
  }

  updateShape(sizePoint) {
    const points = this.slotPoints(sizePoint)
    if(!points) return false

    const {
      topFirst, topSecond, bottomFirst, bottomSecond, outerFirst, outerSecond,
    } = points
    if(this.elements.length == 1) {
      const firstArc = Arc.fromPoints([bottomFirst, outerFirst, topFirst])
      const secondArc = Arc.fromPoints([topSecond, outerSecond, bottomSecond])
      if(!firstArc || !secondArc) return false
      const outline = [
        new Line(topFirst, topSecond),
        secondArc,
        new Line(bottomSecond, bottomFirst),
        firstArc,
      ]
      outline.forEach(element => this.sketch.add(element))
      this.elements = [...outline, this.centerline]
    } else {
      const [top, secondArc, bottom, firstArc] = this.elements
      top.setHandles([topFirst, topSecond])
      bottom.setHandles([bottomSecond, bottomFirst])
      if(!firstArc.setPoints([bottomFirst, outerFirst, topFirst])) return false
      if(!secondArc.setPoints([topSecond, outerSecond, bottomSecond])) return false
    }
    return true
  }

  commit() {
    const [top, secondArc, bottom, firstArc, centerline] = this.elements
    ;[
      [top, 0, firstArc, 2],
      [top, 1, secondArc, 1],
      [bottom, 0, secondArc, 2],
      [bottom, 1, firstArc, 1],
      [centerline, 0, firstArc, 0],
      [centerline, 1, secondArc, 0],
    ].forEach(([line, lineIndex, arc, arcIndex]) => {
      this.sketch.addConstraint(new CoincidentConstraint(
        new ElemRef(line, lineIndex), new ElemRef(arc, arcIndex)
      ))
    })
    this.sketch.addConstraint(new EqualConstraint(firstArc, secondArc))
    ;[top, bottom].forEach(line => {
      this.sketch.addConstraint(new TangentConstraint(line, firstArc))
      this.sketch.addConstraint(new TangentConstraint(line, secondArc))
    })
    if(this.firstCenter.x.almost(this.secondCenter.x) ||
      this.firstCenter.y.almost(this.secondCenter.y)) {
      this.sketch.addConstraint(new HorVertConstraint(
        new ElemRef(firstArc, 0), new ElemRef(secondArc, 0)
      ))
    }
    const radius = firstArc.radius
    if(this.lockedLength !== undefined) {
      const axisCenter = this.firstCenter.clone().lerp(this.secondCenter, 0.5)
      const outside = axisCenter.clone().sub(top.midpoint()).normalize()
      this.sketch.addConstraint(new Dimension(
        [new ElemRef(firstArc, 0), new ElemRef(secondArc, 0)],
        axisCenter.addScaledVector(outside, radius * 2.0),
      ))
    }
    this.addLockedDimension('radius', [firstArc],
      this.firstCenter.clone().addScaledVector(this.slotNormal(), radius * 1.5))
    this.constrainPoint(firstArc, 0, this.firstCenterSnap)
    this.constrainPoint(secondArc, 0, this.secondCenterSnap)
  }

  dispose() {
    this.elements.forEach(element => element.remove())
    this.elements = []
    this.centerline = null
    this.firstCenter = null
    this.secondCenter = null
  }
}


export class PolygonTool extends SketchTool {
  static icon = 'sketch-polygon'
  static hasOptions = true

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    this.sides = 6
    this.lines = []
    this.settings = {
      sides: {
        title: 'Sides',
        type: 'integer',
        min: 3,
        max: 64,
      },
    }
  }

  snapExclusions() { return this.lines }

  setOption(key, value) {
    if(key != 'sides') return
    this.sides = Math.min(64, Math.max(3, Number(value)))
    if(!this.center || !this.radiusPoint) return
    this.rebuildLines()
    this.updatePolygon(this.radiusPoint)
    this.viewport.elementChanged()
  }

  inputFields() {
    if(!this.center) return []
    const point = this.constrained(this.pointer || this.center)
    return [{ key: 'radius', label: 'R', dimension: 'length', value: point.distanceTo(this.center) }]
  }

  constrained(vec) {
    if(!this.center || this.locks.radius === undefined) return vec
    return radialPoint(this.center, vec, this.locks.radius)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    vec = this.constrained(vec)
    const snap = this.pointerSnap()

    if(!this.center) {
      this.center = vec.clone()
    } else {
      if(!this.updatePolygon(vec)) return
      this.commit(snap)
      this.center = null
      this.radiusPoint = null
      this.lines = []
    }
    this.resetInput()
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    this.pointer = vec
    if(!this.center) return
    if(this.updatePolygon(this.constrained(vec))) this.viewport.elementChanged()
  }

  rebuildLines() {
    this.lines.forEach(line => line.remove())
    this.lines = Array.from({ length: this.sides }, () =>
      new Line(this.center.clone(), this.center.clone())
    )
    this.lines.forEach(line => this.sketch.add(line))
  }

  updatePolygon(radiusPoint) {
    if(radiusPoint.distanceToSquared(this.center) < 1e-12) return false
    this.radiusPoint = radiusPoint.clone()
    if(this.lines.length != this.sides) this.rebuildLines()

    const radius = radiusPoint.distanceTo(this.center)
    const startAngle = Math.atan2(
      radiusPoint.y - this.center.y,
      radiusPoint.x - this.center.x,
    )
    const vertices = Array.from({ length: this.sides }, (_value, index) => {
      const angle = startAngle + index * Math.PI * 2 / this.sides
      return new THREE.Vector3(
        this.center.x + Math.cos(angle) * radius,
        this.center.y + Math.sin(angle) * radius,
        this.center.z,
      )
    })
    this.lines.forEach((line, index) => line.setHandles([
      vertices[index], vertices[(index + 1) % vertices.length],
    ]))
    return true
  }

  commit(vertexSnap) {
    this.lines.forEach((line, index) => {
      const next = this.lines[(index + 1) % this.lines.length]
      this.sketch.addConstraint(new CoincidentConstraint(
        new ElemRef(line, 1), new ElemRef(next, 0)
      ))
      if(index) this.sketch.addConstraint(new EqualConstraint(this.lines[0], line))
    })
    if(this.lines.length % 2 == 0) {
      const oppositeOffset = this.lines.length / 2
      this.lines.slice(0, oppositeOffset).forEach((line, index) => {
        this.sketch.addConstraint(new ParallelConstraint(
          line, this.lines[index + oppositeOffset]
        ))
      })
    }
    this.constrainPoint(this.lines[0], 0, vertexSnap)
  }

  dispose() {
    this.lines.forEach(line => line.remove())
    this.lines = []
    this.center = null
    this.radiusPoint = null
  }
}


export class ArcTool extends SketchTool {
  static icon = 'sketch-arc'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)
    if(this.start && this.end) {
      if(this.curve) {
        this.constrainPoint(this.curve, 1, this.startSnap)
        this.constrainPoint(this.curve, 2, this.endSnap)
      }
      this.start = null
      this.end = null
      this.curve = null
      this.startSnap = null
      this.endSnap = null
    } else if(this.start) {
      this.end = vec
      this.endSnap = snap
    } else {
      this.start = vec
      this.startSnap = snap
    }
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    if(!this.start || !this.end) return
    if(!this.curve) {
      this.curve = Arc.fromPoints([this.start, vec, this.end])
      if(!this.curve) return
      this.sketch.add(this.curve)
    } else if(!this.curve.setPoints([this.start, vec, this.end])) {
      return
    }
    this.viewport.elementChanged()
  }

  dispose() {
    if(this.curve) this.curve.remove()
    this.curve = null
    this.start = null
    this.end = null
  }
}


export class ConstraintTool extends HighlightTool {
  constructor(component, viewport, sketch) {
    super(component, viewport, [])
    this.setSelectors(this.constructor.selectors)
    this.sketch = sketch
    this.acceptsInput = item => (!(item instanceof OffsetCurve) || this.constructor.constraintType == TouchConstraint) && this.acceptsItem(item)
    this.items = []
    this.cursor = 'crosshair'
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const curve = await this.getObject(coords)
    if(!curve) {
      this.items = []
      return
    }
    this.pick(curve)
  }

  pick(curve) {
    if(!this.acceptsItem(curve)) {
      window.bus.emit('toast', 'Unsupported geometry for this constraint')
      return
    }
    console.log(curve)
    if(this.items.includes(curve)) return
    this.items.push(curve)
    const counts = {}
    this.selectors.forEach(sel => {
      counts[sel] = this.items.filter(item => (item.mesh ? item.mesh().alcTypes : ['point']).some(t => t == sel ) ).length
    })
    console.log(counts, this.isComplete(counts))
    if(this.isComplete(counts)) {
      if(!this.canConstrain(this.items)) {
        window.bus.emit('toast', 'Invalid geometry combination for this constraint')
        this.items = []
        return
      }
      const constraint = new this.constructor.constraintType(...this.items)
      this.sketch.addConstraint(constraint)
      this.viewport.updateRegions(true)
      this.items = []
    }
  }

  isComplete(counts) {}

  acceptsItem(item) { return true }

  canConstrain(items) { return true }
}

export class TouchConstraintTool extends ConstraintTool {
  static constraintType = TouchConstraint
  static icon = 'constraint-touch'
  static selectors = ['point', 'curve']

  isComplete(counts) { return counts.point == 1 && counts.curve == 1 }

  acceptsItem(item) {
    if(item instanceof ElemRef) {
      return isTouchPoint(item.curve(), item.index)
    }
    return item instanceof Line || item instanceof Circle || item instanceof Arc ||
      (item instanceof OffsetCurve && !!item.geom && !item.invalid)
  }

  canConstrain(items) {
    const point = items.find(item => item instanceof ElemRef)
    const curve = items.find(item => !(item instanceof ElemRef))
    return point && curve && (!(point instanceof ElemRef) || point.curve() != curve)
  }
}

export class HorVertConstraintTool extends ConstraintTool {
  static constraintType = HorVertConstraint
  static icon = 'constraint-horizontal'
  static selectors = ['point', 'axis']

  isComplete(counts) { return counts.point == 2 || (counts.axis == 1 && !counts.point) }
}

export class FixConstraintTool extends ConstraintTool {
  static constraintType = FixConstraint
  static icon = 'lock'
  static selectors = ['point', 'curve']

  isComplete(counts) { return counts.point || counts.curve }
}

export class PerpendicularConstraintTool extends ConstraintTool {
  static constraintType = PerpendicularConstraint
  static icon = 'constraint-perpendicular'
  static selectors = ['curve']

  isComplete(counts) { return counts.curve == 2 }

  acceptsItem(item) { return item instanceof Line || item instanceof Circle || item instanceof Arc }

  canConstrain(items) { return items.some(item => item instanceof Line) }
}

export class ParallelConstraintTool extends ConstraintTool {
  static constraintType = ParallelConstraint
  static icon = 'constraint-parallel'
  static selectors = ['axis']

  isComplete(counts) { return counts.axis == 2 }
}

export class EqualConstraintTool extends ConstraintTool {
  static constraintType = EqualConstraint
  static icon = 'equals'
  static selectors = ['curve']

  isComplete(counts) { return counts.curve == 2 }

  acceptsItem(item) { return item instanceof Line || item instanceof Circle || item instanceof Arc }

  canConstrain(items) {
    return items.every(item => item instanceof Line) ||
      items.every(item => item instanceof Circle || item instanceof Arc)
  }
}

export class ConcentricConstraintTool extends ConstraintTool {
  static constraintType = ConcentricConstraint
  static icon = 'constraint-concentric'
  static selectors = ['curve']

  isComplete(counts) { return counts.curve == 2 }

  acceptsItem(item) { return item instanceof Circle || item instanceof Arc }
}

export class TangentConstraintTool extends ConstraintTool {
  static constraintType = TangentConstraint
  static icon = 'constraint-tangent'
  static selectors = ['curve']

  isComplete(counts) { return counts.curve == 2 }

  acceptsItem(item) {
    return item instanceof Line || item instanceof Circle || item instanceof Arc || item instanceof Spline
  }

  canConstrain(items) {
    const round = item => item instanceof Circle || item instanceof Arc
    return (items.filter(item => item instanceof Line).length == 1 &&
      items.filter(item => round(item) || item instanceof Spline).length == 1) ||
      items.every(round) || items.every(item => item instanceof Spline) ||
      (items.some(item => item instanceof Spline) && items.some(round))
  }
}

const mirrorable = elem => elem instanceof SketchPoint || elem instanceof Line || elem instanceof Circle || elem instanceof Arc

function sameItem(left, right) {
  if(!(left instanceof ElemRef) || !(right instanceof ElemRef)) return toRaw(left) == toRaw(right)
  return toRaw(left.curve()) == toRaw(right.curve()) && left.index == right.index
}

// Picks the mirror line first, then any number of symmetric pairs.
export class SymmetricConstraintTool extends ConstraintTool {
  static constraintType = SymmetricConstraint
  static icon = 'constraint-symmetric'
  static selectors = ['point', 'curve']

  acceptsItem(item) {
    const [mirror, first] = this.items
    const curve = item instanceof ElemRef ? item.curve() : item
    if(!mirror) return !(item instanceof ElemRef) && mirrorable(curve)
    if(toRaw(curve) == toRaw(mirror) || (first && sameItem(item, first))) return false
    if(first) return first instanceof ElemRef ? item instanceof ElemRef : item.constructor == first.constructor
    return item instanceof ElemRef || (mirrorable(item) && !(item instanceof SketchPoint))
  }

  pick(item) {
    const [mirror, first] = this.items
    if(!mirror) {
      if(item instanceof Line) this.items = [item]
      else window.bus.emit('toast', 'Select the mirror line first')
      return
    }
    if(!first) {
      this.items = [mirror, item]
      return
    }
    this.sketch.addConstraint(new SymmetricConstraint(mirror, first, item))
    this.viewport.updateRegions(true)
    this.items = [mirror]
  }
}

function mirroredElement(elem, mirror) {
  const reflect = point => mirror.reflect(point)
  if(elem instanceof SketchPoint) return new SketchPoint(reflect(elem.point))
  if(elem instanceof Line) return new Line(...elem.points.map(reflect))
  if(elem instanceof Circle) return new Circle(reflect(elem.center()), elem.radius)
  // Reflection reverses the traversal direction of arcs.
  const direction = mirror.direction()
  const angle = Math.atan2(direction.y, direction.x)
  return new Arc(reflect(elem.center()), elem.radius, 2 * angle - elem.start, -elem.sweep)
}

// Picks the mirror line first, then mirrors the elements selected beforehand,
// followed by each element clicked afterwards.
export class SketchMirrorTool extends HighlightTool {
  static icon = 'sketch-mirror'

  constructor(component, viewport, sketch) {
    super(component, viewport, ['curve', 'point'])
    this.sketch = sketch
    this.localSpace = true
    this.cursor = 'crosshair'
    this.items = []
    this.preselection = viewport.document.selection.items.filter(item => this.acceptsInput(item))
  }

  acceptsInput(item) {
    const elem = item instanceof ElemRef ? item.curve() : item
    return mirrorable(elem) && elem.sketch == this.sketch && toRaw(elem) != toRaw(this.items[0]) &&
      (!(item instanceof ElemRef) || elem instanceof SketchPoint)
  }

  // A point element is offered both as itself and as its handle.
  orderItems(items) {
    return [...new Set(items.map(item => toRaw(item instanceof ElemRef ? item.curve() : item)))]
  }

  async click(vec, coords) {
    const item = await this.getObject(coords)
    if(!item) {
      this.items = []
      return
    }
    if(this.items.length) return this.mirror([item])
    if(!(item instanceof Line)) return window.bus.emit('toast', 'Select the mirror line first')
    this.items = [item]
    if(!this.preselection.length) return
    const copies = this.mirror(this.preselection.filter(elem => toRaw(elem) != toRaw(item)))
    const selection = this.viewport.document.selection
    selection.clear()
    copies.forEach(copy => selection.add(copy))
    this.preselection = []
  }

  mirror(elements) {
    const [mirror] = this.items
    const copies = elements.map(elem => {
      const copy = mirroredElement(elem, mirror)
      copy.isReference = elem.isReference
      this.sketch.add(copy)
      this.sketch.addConstraint(elem instanceof SketchPoint ?
        new SymmetricConstraint(mirror, new ElemRef(elem, 0), new ElemRef(copy, 0)) :
        new SymmetricConstraint(mirror, elem, copy))
      return copy
    })
    // Symmetry already joins copies wherever their originals meet. Points on
    // the mirror line additionally have to stay there to keep each original
    // joined to its own image.
    elements.filter(elem => !elem.projection).forEach(elem => elem.handles().forEach((point, index) => {
      if(!isTouchPoint(elem, index) || !mirror.reflect(point).almost(point)) return
      const onMirror = this.sketch.constraints.some(constraint =>
        (constraint instanceof TouchConstraint || constraint instanceof MidpointConstraint) &&
        constraint.items.some(other => other.index === undefined && toRaw(other.curve()) == toRaw(mirror)) &&
        constraint.items.some(other => other.index !== undefined &&
          this.sketch.coincidentPointsConnected(elem, index, other.curve(), other.index))
      ) || [0, 1].some(end => this.sketch.coincidentPointsConnected(elem, index, mirror, end))
      if(!onMirror) this.sketch.addConstraint(new TouchConstraint(new ElemRef(elem, index), mirror))
    }))
    this.viewport.updateRegions(true)
    return copies
  }
}

export class OffsetTool extends HighlightTool {
  static icon = 'sketch-offset'

  constructor(component, viewport, sketch) {
    super(component, viewport, ['curve', 'profile'])
    this.sketch = sketch
    this.localSpace = true
    this.cursor = 'crosshair'
  }

  acceptsInput(item) {
    return item.sketch == this.sketch && (supportsOffset(item) ||
      (item instanceof Profile && item.rings.every(ring => ring.segments.every(supportsOffset))))
  }

  orderItems(items) {
    return items.some(supportsOffset) ? items.filter(supportsOffset) : items
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(!vec) return
    if(!this.source) {
      this.source = await this.getObject(coords)
      if(!this.source) return
      try {
        const elements = [...this.sketch.elements, ...this.sketch.projectedElements()]
        this.groups = this.source instanceof Profile ? profileOffsetGroups(this.source, elements) :
          [connectedOffsetChain(this.source, elements)]
        this.items = this.groups.flat().map(entry => entry.curve)
      } catch(error) {
        window.bus.emit('toast', error.message)
        this.source = null
      }
      return
    }
    if(!this.updatePreview(vec)) {
      if(this.previewError) window.bus.emit('toast', this.previewError)
      return
    }
    this.sketch.addConstraint(this.constraint)
    this.constraint.active = true
    this.source = null
    this.curve = null
    this.curves = []
    this.groups = null
    this.items = []
    this.constraint = null
    this.viewport.dimensionPreview = null
    this.viewport.updateRegions(true)
  }

  mouseMove(vec, coords) {
    if(!this.source) return super.mouseMove(vec, coords)
    if(vec) this.updatePreview(vec)
  }

  updatePreview(position) {
    this.clearPreview()
    const source = this.source
    if(source instanceof Profile || source instanceof Spline || this.groups.flat().length > 1) {
      try {
        const frame = nearestOffsetFrame(this.groups, position)
        const distance = position.clone().sub(frame.point).dot(frame.normal)
        this.curves = buildOffsetCurves(this.groups, distance)
        this.curves.forEach(curve => {
          curve.isReference = this.groups[0][0].curve.isReference
          this.sketch.add(curve)
        })
        this.constraint = new ProfileOffsetConstraint(this.groups, this.curves, position.clone(), distance)
      } catch(error) {
        this.clearPreview()
        this.previewError = error.message || 'Offset could not be created'
        return false
      }
    } else if(source instanceof Line) {
      const direction = source.direction().normalize()
      const normal = new THREE.Vector3(-direction.y, direction.x, 0)
      const distance = position.clone().sub(source.points[0]).dot(normal)
      if(!Number.isFinite(distance) || Math.abs(distance) <= EPSILON || source.length() <= EPSILON) return false
      const translation = normal.multiplyScalar(distance)
      this.curve = new Line(...source.points.map(point => point.clone().add(translation)))
    } else {
      const radius = position.distanceTo(source.center())
      if(!Number.isFinite(radius) || radius <= EPSILON || Math.abs(radius - source.radius) <= EPSILON) return false
      this.curve = source instanceof Circle ? new Circle(source.center(), radius) :
        Arc.fromPoints([0, 0.5, 1].map(u => source.sample(u).sub(source.center())
          .multiplyScalar(radius / source.radius).add(source.center())))
      if(!this.curve) return false
    }
    if(this.curve) {
      this.curve.isReference = source.isReference
      this.sketch.add(this.curve)
      this.constraint = new OffsetConstraint([source, this.curve], position.clone())
    }
    this.previewError = null
    this.constraint.sketch = this.sketch
    this.viewport.dimensionPreview = this.constraint
    this.viewport.renderer.render()
    return true
  }

  clearPreview() {
    const curves = [...(this.curves || []), ...(this.curve ? [this.curve] : [])]
    curves.forEach(curve => {
      this.sketch.remove(curve)
      curve.clear()
    })
    this.curves = []
    this.curve = null
    this.constraint = null
    this.viewport.dimensionPreview = null
    this.viewport.renderer.render()
  }

  dispose() {
    this.clearPreview()
    this.source = null
    this.groups = null
    this.items = []
  }
}

export class DimensionTool extends HighlightTool {
  static icon = 'dimension'

  acceptsInput(item) {
    const isPoint = item instanceof ElemRef && item.index !== undefined
    const first = this.items[0]
    if(first instanceof ElemRef) return isPoint || item instanceof Circle
    if(first instanceof Line) return item instanceof Line
    if(first instanceof Circle) {
      return item instanceof Circle || item instanceof Arc || (isPoint && item.curve() != first)
    }
    if(first instanceof Arc) return item instanceof Circle || item instanceof Arc
    return isPoint || item instanceof Line || item instanceof Circle || item instanceof Arc
  }

  constructor(component, viewport, sketch) {
    super(component, viewport, ['curve', 'point'])
    this.sketch = sketch
    this.items = []
    this.cursor = 'crosshair'
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)

    if(this.items.length == 0 ||
      (!this.isReadyToPlace() && !(this.items.length == 1 && this.items[0] instanceof Circle))) {
      const curve = await this.getObject(coords)
      if(!curve) return
      this.items.push(curve)
      if(this.canPlaceDimension()) this.updatePreview(vec)

    } else if(this.items.length == 1 && this.items[0] instanceof Line) {
      const curve = await this.getObject(coords)
      if(curve instanceof Line && curve != this.items[0]) {
        this.items.push(curve)
        this.updatePreview(vec)
      } else {
        this.placeDimension(vec)
      }

    } else if(this.items.length == 1 && (this.items[0] instanceof Circle || this.items[0] instanceof Arc)) {
      // A second circle or arc dimensions the radial offset between them
      const curve = await this.getObject(coords)
      if(curve && curve != this.items[0]) {
        this.items.push(curve)
        this.updatePreview(vec)
      } else {
        this.placeDimension(vec)
      }

    } else {
      this.placeDimension(vec)
    }
  }

  async mouseMove(vec, coords) {
    if(!this.canPlaceDimension()) return super.mouseMove(vec, coords)
    if(this.items.length == 1 &&
      (this.items[0] instanceof Line || this.items[0] instanceof Circle || this.items[0] instanceof Arc)) {
      // Hide the label while hovering a valid second element
      const hovered = await super.mouseMove(vec, coords)
      if(hovered && hovered != this.items[0]) return this.clearPreview()
    }
    this.updatePreview(vec)
  }

  canPlaceDimension() {
    return this.isReadyToPlace() ||
      (this.items.length == 1 && this.items[0] instanceof Circle)
  }

  isReadyToPlace() {
    return (
      this.items.length == 2 ||
      this.items[0] instanceof Line ||
      this.items[0] instanceof Arc
    )
  }

  placeDimension(position) {
    const toSketch = this.sketch.workplane.clone().invert()
    const constraint = new Dimension(this.items, position.clone().applyMatrix4(toSketch))
    constraint.active = true
    this.sketch.addConstraint(constraint)
    this.viewport.updateRegions(true)
    this.items = []
    this.clearPreview()
  }

  updatePreview(position) {
    const toSketch = this.sketch.workplane.clone().invert()
    const constraint = new Dimension(this.items, position.clone().applyMatrix4(toSketch))
    constraint.sketch = this.sketch
    this.viewport.dimensionPreview = constraint
    this.viewport.renderer.render()
  }

  clearPreview() {
    this.viewport.dimensionPreview = null
    this.viewport.renderer.render()
  }

  dispose() {
    this.items = []
    this.clearPreview()
  }
}
