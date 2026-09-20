import * as THREE from 'three'
import { markRaw } from 'vue'

// import {
//   vecToThree,
//   matrix2three,
//   matrixFromThree,
//   rotationFromNormal
// } from './utils.js'

import {
  Line,
  Circle,
  Arc,
  Spline,
  OffsetCurve,
  SketchPoint,
} from './core/geom2d.js'

import {
  Edge,
  Profile,
  Solid,
  getViewportTessellationTolerance,
} from './core/geom3d.js'
import { PlaneHelper, PointHelper } from './core/helpers.js'

import {
  CoincidentConstraint,
  TouchConstraint,
  MidpointConstraint,
  PerpendicularConstraint,
  HorVertConstraint,
  ParallelConstraint,
  EqualConstraint,
  TangentConstraint,
  FixConstraint,
  Dimension,
  OffsetConstraint,
  ProfileOffsetConstraint,
  Projection,
  ProjectedPoint,
  SketchOrigin,
  ElemRef,
} from './core/sketch.js'
import {
  solveAssembly,
  worldTransform,
} from './core/assembly.js'
import { EPSILON } from './core/utils.js'
import materials, { DRAFT_DIRECTIONS } from './materials.js'
import { supportsOffset, connectedOffsetChain, profileOffsetGroups, nearestOffsetFrame, buildOffsetCurves } from './core/sketch-offset.js'

const dragThreshold = 2
const tangentSnapAngle = THREE.MathUtils.degToRad(7.5)
const immediateSelectionVolumeRatio = 8

function boundingBoxVolume(solid) {
  const bounds = new THREE.Box3()
  solid.faces().forEach(face => {
    const mesh = face.mesh?.()
    if(mesh) bounds.expandByObject(mesh)
  })
  if(bounds.isEmpty()) return
  const size = bounds.getSize(new THREE.Vector3())
  return size.x * size.y * size.z
}


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
    if(!vec || !lastCoords ||
      coords.distanceTo(lastCoords) > dragThreshold) return this.viewport.renderer.render()
    this.click(vec, coords)
    // window.gc && window.gc()
  }

  cancelPointer() { this.lastCoords = null }

  mouseMove(vec, coords) {}

  guideSnapPoints() { return [] }

  snapExclusions() { return this.curve ? [this.curve] : [] }

  acceptsSnapElement(_elem) { return true }

  dispose() {}
}

function captureSnap(snapper) {
  const { curve, pointTarget, intersection, point, x, y } = snapper.snapped || {}
  if(!(curve || pointTarget || intersection || x || y)) return
  return { ...pointTarget, curve, intersection, point, x, y }
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
    ...sketch.projections.map(projection => projection.geometry()).filter(Boolean),
  ]
  return candidates.flatMap(candidate =>
    candidate == excludedElem || !candidate.handles ? [] : candidate.handles().map((point, index) => ({
      elem: candidate,
      index,
      point,
    }))
  ).filter(target =>
    isConstrainablePoint(target.elem, target.index) && target.point.almost(position)
  )
}

function constrainCoincidentPoints(sketch, elem, index, targetElem, targetIndex) {
  if(elem instanceof SketchPoint && sketch.dissolvePoint(elem, targetElem, targetIndex)) return elem
  if(targetElem instanceof SketchPoint && sketch.dissolvePoint(targetElem, elem, index)) return targetElem
  return sketch.addConstraint(
    new CoincidentConstraint(new ElemRef(elem, index), new ElemRef(targetElem, targetIndex))
  )
}

function addCapturedSnapConstraint(sketch, snap, elem, index) {
  if(!isConstrainablePoint(elem, index)) return

  // Explicit points retain priority even if another overlapping target won
  // the screen-space snap tie.
  const position = elem.handles()[index]
  const positionTargets = pointTargetsAtPosition(sketch, position, elem)
  const explicitTarget = positionTargets.find(target => target.elem instanceof SketchPoint)
  if(explicitTarget) return constrainCoincidentPoints(
    sketch, elem, index, explicitTarget.elem, explicitTarget.index
  )

  if(snap && snap.intersection) {
    const point = sketch.findOrCreateIntersectionPoint(snap.intersection, snap.point)
    if(point == elem) return point
    return constrainCoincidentPoints(sketch, elem, index, point, 0)
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
    return constrainCoincidentPoints(sketch, elem, index, snap.elem, snap.index)
  }

  // Only use another coincident handle as a positional fallback when no
  // captured target was available. Otherwise an already-connected endpoint
  // can mask the point the user just snapped to.
  const positionTarget = positionTargets[0]
  if(positionTarget) return constrainCoincidentPoints(
    sketch, elem, index, positionTarget.elem, positionTarget.index
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
        ...sketch.projections.map(projection => projection.geometry()).filter(Boolean),
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
    const stress = simulation.stressAt(position)
    if(stress !== undefined) return `Relative stress ${(stress * 100).toFixed(0)}%`
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
          top: 'Top',
          front: 'Front',
          side: 'Side',
        },
        visible: tool => tool.diagnosticMode == 'draft',
      },
      zebraFrequency: {
        title: 'Frequency',
        type: 'integer',
        min: 1,
        max: 64,
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
  }

  getObject(coords, any) {
    return new Promise(resolve => {
      let items = this.viewport.renderer
        .objectsAtScreen(coords, this.realSelectors, this.includeInactive)
        .flatMap(obj => {
          if(!obj.alcTypes.some(type => type == 'face')) return obj.alcObject
          const mapped = []
          if(this.selectors.includes('solid')) mapped.push(obj.alcObject.solid)
          if(this.selectors.includes('component')) mapped.push(obj.alcObject.solid.component)
          if(mapped.length) return mapped
          return obj.alcObject
        })
      items = Array.from(new Set(items))
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
    this.setSelectors(this.viewport.document.activeSketch ? ['curve', 'point'] : ['curve', 'solid'])
  }

  shouldPickImmediately(items) {
    // Items retain raycast order, so the first solid has the nearest hit.
    const [closest, ...others] = items
    if(!(closest instanceof Solid) || others.some(item => !(item instanceof Solid))) return false

    const closestVolume = boundingBoxVolume(closest)
    if(!Number.isFinite(closestVolume) || closestVolume <= EPSILON) return false
    return others.every(item => {
      const volume = boundingBoxVolume(item)
      return Number.isFinite(volume) && volume >= closestVolume * immediateSelectionVolumeRatio
    })
  }

  async click(vec, coords) {
    const curve = await this.getObject(coords)
    if(curve) {
      this.viewport.document.selection.handle(curve, this.viewport.bus.isCtrlPressed)
    } else {
      if(this.viewport.bus.isCtrlPressed) return this.viewport.renderer.render()
      this.viewport.document.selection.clear()
    }
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    this.pointerDown = true
    // const sel = this.viewport.document.selection.items[0]
    // A drag must start from the front-most hit immediately. Opening the
    // overlap picker here leaves mouseDown pending and prevents mouseMove
    // from ever receiving a solid to manipulate. Click selection opens the
    // picker once from click() after mouseUp instead.
    const object = await this.getObject(coords, true)
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
          radius: elem.radius,
        }))
        : null
      this.curveDrag = {
        elem: object,
        pointer: vec.clone(),
        coords: coords.clone(),
        handles: object.handles().map(handle => handle.clone()),
        group,
      }
      // Reuse the solver's temporary pointer-driving constraint. Lines use
      // one endpoint while circles and arcs use their center; the remaining
      // parameters start from the dragged geometry and are then allowed to
      // respond to the sketch's constraints.
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
    this.snapToPoints = !this.curveDrag
    this.cursor = 'grabbing'
  }

  mouseUp(vec, coords) {
    this.pointerDown = false
    const boxDrag = this.boxDrag
    const boxSelecting = boxDrag && coords.distanceTo(boxDrag.start) > dragThreshold
    // this.mouseMove(vec, coords)
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
      addCapturedSnapConstraint(sketch, captureSnap(this.viewport.snapper), handle.elem, handle.index)
      const replacement = handle.elem.dissolvedTo
      if(replacement && !sketch.elements.includes(handle.elem)) {
        // Viewport keeps this handle for the post-drag solve. Redirect it so
        // the solver does not receive the explicit point that was removed.
        handle.elem = replacement.elem
        handle.index = replacement.index
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
        this.curveDrag.group.forEach(item => {
          const handles = item.handles.map(point => point.clone().add(delta))
          if(item.elem instanceof Arc) {
            item.elem.setSolvedHandles(handles, item.radius)
          } else {
            item.elem.setHandles(handles, false)
          }
        })
      } else if(handle.elem instanceof Line) {
        const delta = vec.clone().sub(this.curveDrag.pointer)
        handle.elem.setHandles(originalHandles.map(point => point.clone().add(delta)), false)
      } else {
        const center = originalHandles[0].clone()
        const radius = center.distanceTo(vec)
        if(radius <= EPSILON) return
        if(handle.elem instanceof Arc) {
          const endpoints = originalHandles.slice(1).map(point =>
            point.clone().sub(center).normalize().multiplyScalar(radius).add(center)
          )
          handle.elem.setSolvedHandles([center, ...endpoints], radius)
        } else {
          handle.elem.setHandles([center, vec.clone()], false)
        }
      }
      this.viewport.updateSketch(true)

    // Drag Handles
    } else if(handle) {
      if(handle.elem instanceof Arc && handle.index > 0) {
        handle.elem.setEndpoint(handle.index, vec)
      } else {
        let handles = handle.elem.handles()
        handles[handle.index] = vec//.toArray()
        handle.elem.setHandles(handles, false)
      }
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


export class ProjectTool extends HighlightTool {
  static icon = 'layer-group'

  constructor(component, viewport, sketch) {
    super(component, viewport, ['point', 'edge'])
    this.sketch = sketch
    this.localSpace = true
    this.includeInactive = true
    this.acceptsInput = item =>
      item instanceof Edge || item instanceof PointHelper || item instanceof ElemRef
    this.cursor = 'copy'
  }

  async click(vec, coords) {
    const edge = await this.getObject(coords)
    if(!edge) return
    const projection = new Projection(edge)
    this.sketch.addProjection(projection)
    this.viewport.updateRegions(true)
  }
}


// export class PlaneTool extends HighlightTool {
//   constructor(component, viewport) {
//     super(component, viewport, ['face'])
//   }

//   click(vec, coords) {
//     const face = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
//     if(face && face.alcObject.surface_type() == 'Planar') {
//       const position = vecToThree(face.alcObject.origin())
//       let rotation = rotationFromNormal(vecToThree(face.alcObject.normal()))

//       this.viewport.renderer.sketchPlane.position = position
//       this.viewport.renderer.sketchPlane.rotation.setFromRotationMatrix(rotation)

//       rotation.setPosition(position)
//       this.viewport.snapper.planeTransform = rotation
//       this.component.real.sketch().set_workplane(matrixFromThree(rotation))

//       this.viewport.regionsDirty = true
//       this.viewport.updateRegions()
//     }
//     this.viewport.renderer.render()
//   }

//   mouseDown(vec, coords) {
//     const face = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
//     this.viewport.renderer.render()
//   }
// }


export class TrimTool extends Tool {
  static icon = 'route'

  constructor(component, viewport, sketch) {
    super(component, viewport)
    this.sketch = sketch
    this.localSpace = true
  }

  curveAt(coords) {
    const object = this.viewport.renderer.objectsAtScreen(coords, ['curve'])[0]
    const curve = object && object.alcObject
    return curve && curve.sketch == this.sketch && !curve.projection && !(curve instanceof OffsetCurve) ? curve : null
  }

  click(vec, coords) {
    const curve = this.curveAt(coords)
    if(!curve) return this.viewport.renderer.render()

    const pieces = curve.split(this.sketch.elements)
    if(pieces.length < 2) {
      window.bus.emit('toast', 'Curve has no intersection to trim to')
      return this.viewport.renderer.render()
    }

    const removed = pieces.minMaxBy(Math.min, piece => piece.distanceTo(vec))
    const replacements = pieces.filter(piece => piece != removed)
    this.sketch.replaceElement(curve, replacements)
    removed.clear()
    this.viewport.document.selection.delete(curve)
    this.viewport.$emit('update:highlight', null)
    this.viewport.updateRegions(true)
    this.viewport.renderer.render()
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
  }

  mouseMove(vec, coords) {
    const curve = this.curveAt(coords)
    this.viewport.$emit('update:highlight', curve)
  }
}


class PickTool extends HighlightTool {
  constructor(component, viewport, selectors, callback) {
    super(component, viewport, selectors)
    this.callback = callback
    this.localSpace = false
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
  }

  constrainPoint(elem, index, snap) {
    if(arguments.length < 3) snap = captureSnap(this.viewport.snapper)
    const captured = addCapturedSnapConstraint(this.sketch, snap, elem, index)
    const origin = this.originSnapConstraint(elem, index, snap)
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
    const origin = this.sketch.originPoint()
    const { x, y } = this.viewport.snapper.snapped || {}
    return origin && x && y && x.almost(origin) && y.almost(origin)
  }
}


export class LineTool extends SketchTool {
  static icon = 'pen'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  constrainCompletedLine(line, endSnap) {
    this.constrainPoint(line, 1, endSnap)
    addInferredTangentConstraint(this.sketch, line, endSnap)
    addInferredTangentConstraint(this.sketch, line, this.startSnap, 0)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    this.mouseMove(vec)
    const snap = captureSnap(this.viewport.snapper)

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
        if(xSnapped || ySnapped) {
          // const type = (xSnapped ? HorizontalConstraint : VerticalConstraint)
          const type = HorVertConstraint
          this.sketch.addConstraint(new type(old))
        }
      }
      this.startSnap = snap
    }
  }

  mouseMove(vec) {
    if(!this.curve) return
    let p1 = this.curve.endpoints()[0]
    this.curve.setHandles([p1, vec], false)
    this.viewport.elementChanged()
  }

  guideSnapPoints() {
    return this.curve ? [this.curve.endpoints()[0]] : []
  }

  dispose() {
    if(!this.curve) return
    this.curve.remove()
  }
}


export class PointTool extends SketchTool {
  static icon = 'asterisk'

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
  static icon = 'route'
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
    spline.preserveGeometry = false
    spline.setHandles(points)
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
    spline.preserveGeometry = false
    spline.setHandles(points)
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
  static icon = 'ban'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.center) {
      this.center = null
      this.curve = null
    } else {
      this.center = vec
      // this.curve = this.sketch.add_circle(vec.toArray(), 1)
      // this.curve = this.sketch.add_circle(vec, 1)
      this.curve = new Circle(vec, 1.0)
      this.sketch.add(this.curve)
      this.constrainPoint(this.curve, 0)
      // this.curve.sketch = this.sketch
    }
  }

  mouseMove(vec) {
    if(!this.center) return
    // this.curve.setHandles([this.center.toArray(), vec.toArray()], false)
    this.curve.setHandles([this.center, vec], false)
    this.viewport.elementChanged()
  }
}


export class RectangleTool extends SketchTool {
  static icon = 'vector-square'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    this.lines = []
  }

  snapExclusions() { return this.lines }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)

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
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    if(!this.corner) return

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
    })
    ;[bottom, right, top, left].forEach(line => {
      this.sketch.addConstraint(new HorVertConstraint(line))
    })

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
  static icon = 'capsules'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
    this.elements = []
  }

  snapExclusions() { return this.elements }

  guideSnapPoints() {
    return this.firstCenter && !this.secondCenter ? [this.firstCenter] : []
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)

    if(!this.firstCenter) {
      this.firstCenter = vec.clone()
      this.firstCenterSnap = snap
    } else if(!this.secondCenter) {
      if(vec.distanceToSquared(this.firstCenter) < 1e-12) return
      this.secondCenter = vec.clone()
      this.secondCenterSnap = snap
    } else {
      if(!this.updateShape(vec)) return
      this.commit()
      this.firstCenter = null
      this.secondCenter = null
      this.firstCenterSnap = null
      this.secondCenterSnap = null
      this.elements = []
    }
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    if(!this.firstCenter || !this.secondCenter) return
    if(this.updateShape(vec)) this.viewport.elementChanged()
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
    if(!this.elements.length) {
      const firstArc = Arc.fromPoints([bottomFirst, outerFirst, topFirst])
      const secondArc = Arc.fromPoints([topSecond, outerSecond, bottomSecond])
      if(!firstArc || !secondArc) return false
      this.elements = [
        new Line(topFirst, topSecond),
        secondArc,
        new Line(bottomSecond, bottomFirst),
        firstArc,
      ]
      this.elements.forEach(element => this.sketch.add(element))
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
    const [top, secondArc, bottom, firstArc] = this.elements
    ;[
      [top, 0, firstArc, 2],
      [top, 1, secondArc, 1],
      [bottom, 0, secondArc, 2],
      [bottom, 1, firstArc, 1],
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
    this.constrainPoint(firstArc, 0, this.firstCenterSnap)
    this.constrainPoint(secondArc, 0, this.secondCenterSnap)
  }

  dispose() {
    this.elements.forEach(element => element.remove())
    this.elements = []
    this.firstCenter = null
    this.secondCenter = null
  }
}


export class PolygonTool extends SketchTool {
  static icon = 'draw-polygon'
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

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)

    if(!this.center) {
      this.center = vec.clone()
    } else {
      if(!this.updatePolygon(vec)) return
      this.commit(snap)
      this.center = null
      this.radiusPoint = null
      this.lines = []
    }
    this.viewport.elementChanged()
  }

  mouseMove(vec) {
    if(!this.center) return
    if(this.updatePolygon(vec)) this.viewport.elementChanged()
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
  static icon = 'bezier-curve'

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
  static icon = 'asterisk'
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
  static icon = 'ruler-horizontal'
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
  static icon = 'angle-up'
  static selectors = ['axis']

  isComplete(counts) { return counts.axis == 2 }
}

export class ParallelConstraintTool extends ConstraintTool {
  static constraintType = ParallelConstraint
  static icon = 'exchange-alt'
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

export class TangentConstraintTool extends ConstraintTool {
  static constraintType = TangentConstraint
  static icon = 'bezier-curve'
  static selectors = ['curve']

  isComplete(counts) { return counts.curve == 2 }

  acceptsItem(item) {
    return item instanceof Line || item instanceof Circle || item instanceof Arc || item instanceof Spline
  }

  canConstrain(items) {
    return (items.filter(item => item instanceof Line).length == 1 &&
      items.filter(item => item instanceof Circle || item instanceof Arc || item instanceof Spline).length == 1) ||
      (items.filter(item => item instanceof Circle).length == 2) ||
      (items.filter(item => item instanceof Circle || item instanceof Arc || item instanceof Spline).length == 2 &&
        items.some(item => item instanceof Spline) &&
        items.some(item => item instanceof Circle || item instanceof Arc))
  }
}

export class OffsetTool extends HighlightTool {
  static icon = 'layer-group'

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
        const elements = [...this.sketch.elements, ...this.sketch.projections.map(projection => projection.geometry()).filter(Boolean)]
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
  static icon = 'ruler'

  acceptsInput(item) { return !(item instanceof OffsetCurve) }

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
      // if(!curve || !(curve instanceof Line)) {
      //   this.items = []
      //   return
      // }
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

    } else if(this.items.length == 1 && this.items[0] instanceof Circle) {
      const curve = await this.getObject(coords)
      if(curve && curve != this.items[0] &&
        (curve instanceof Circle || (curve instanceof ElemRef && curve.curve() != this.items[0]))) {
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
      (this.items[0] instanceof Line || this.items[0] instanceof Circle)) {
      await super.mouseMove(vec, coords)
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
