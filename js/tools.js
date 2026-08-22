import * as THREE from 'three'

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
  SketchPoint,
} from './core/geom2d.js'

import {
  Edge,
  Solid,
} from './core/geom3d.js'
import { PointHelper } from './core/helpers.js'

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
  Projection,
  ProjectedPoint,
  SketchOrigin,
  ElemRef,
} from './core/sketch.js'
import { solveAssembly, worldTransform } from './core/assembly.js'

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
    if(!this.lastCoords ||
      coords.distanceTo(this.lastCoords) > dragThreshold) return this.viewport.renderer.render()
    this.click(vec, coords)
    // window.gc && window.gc()
  }

  mouseMove(vec, coords) {}

  guideSnapPoints() { return [] }

  snapExclusions() { return this.curve ? [this.curve] : [] }

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


export class ThermalProbeTool extends Tool {
  constructor(component, viewport) {
    super(component, viewport)
    this.cursor = 'crosshair'
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
    const temperature = simulation.temperatureAt(position)
    this.viewport.thermalProbe = temperature === undefined ? null : {
      position: coords,
      temperature,
    }
  }

  dispose() {
    this.viewport.thermalProbe = null
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

  async mouseMove(vec, coords) {
    if(this.viewport.hoveredHandle || this.viewport.hoveredDimension) return this.viewport.renderer.render()
    if(this.viewport.pickingPath) this.viewport.updatePath(this.viewport.pickingPath)
    const object = await this.getObject(coords, true)
    this.viewport.$emit('update:highlight', object)
    if(object && object.id) console.log(object.id)
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
        // .filter(obj => this.viewport.transloader.isActive(obj) )
      items = Array.from(new Set(items))
      const handle = this.viewport.hoveredHandle
      if(handle && this.realSelectors.includes('point') && !this.preferPointObjects) {
        items.unshift(new ElemRef(handle.elem, handle.index))
      }
      const acceptsInput = this.acceptsInput ||
        this.viewport.document.activeFeature?.acceptsInput?.bind(this.viewport.document.activeFeature)
      if(acceptsInput) items = items.filter(item => acceptsInput(item))
      if(items.length > 1 && !any) {
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
      this.startPoses = new Map(
        this.viewport.document.top().getChildren().map(component => [
          component.id,
          component.transform && component.transform.clone(),
        ])
      )
    }
    console.log(this.object)
    if(!this.viewport.activeHandle && !this.viewport.activeDimension) return
    this.snapToPoints = true
    this.cursor = 'grabbing'
  }

  mouseUp(vec, coords) {
    this.pointerDown = false
    // this.mouseMove(vec, coords)
    super.mouseUp(vec, coords)
    this.snapToPoints = false
    this.cursor = 'auto'
    delete this.object
    const sketch = this.viewport.document.activeSketch
    const handle = this.viewport.activeHandle
    if(sketch && handle) {
      addCapturedSnapConstraint(sketch, captureSnap(this.viewport.snapper), handle.elem, handle.index)
      const replacement = handle.elem.dissolvedTo
      if(replacement && !sketch.elements.includes(handle.elem)) {
        // Viewport keeps this handle for the post-drag solve. Redirect it so
        // the solver does not receive the explicit point that was removed.
        handle.elem = replacement.elem
        handle.index = replacement.index
      }
    }
  }

  mouseMove(vec, coords) {
    const handle = this.viewport.activeHandle
    const dimension = this.viewport.activeDimension

    // Drag Handles
    if(handle) {
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
      const cameraMatrix = this.viewport.renderer.camera.matrixWorld
      const right = new THREE.Vector3().setFromMatrixColumn(cameraMatrix, 0)
      const up = new THREE.Vector3().setFromMatrixColumn(cameraMatrix, 1)

      const scale = 0.5
      const diffX = coords.x - this.startCoords.x
      const diffY = coords.y - this.startCoords.y
      const translation = right.multiplyScalar(diffX * scale)
        .add(up.multiplyScalar(-diffY * scale))
      const desiredWorld = new THREE.Matrix4().makeTranslation(translation).multiply(this.startWorld)

      const comp = this.object.component
      this.viewport.document.top().getChildren().forEach(component => {
        const pose = this.startPoses.get(component.id)
        component.transform = pose && pose.clone()
      })
      solveAssembly(this.viewport.document.top(), comp, desiredWorld)

    } else {
      super.mouseMove(vec, coords)
    }
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
    return curve && curve.sketch == this.sketch && !curve.projection ? curve : null
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

  async mouseDown(vec, coords, event) {
    super.mouseDown(vec, coords)
    const repick = event.ctrlKey || event.metaKey
    const object = await this.getObject(coords, !this.chooseAmongItems)
    if(!object) return
    this.callback(object, repick)
  }
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

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const snap = captureSnap(this.viewport.snapper)
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

  mouseMove(vec) {
    if(!this.curve) return
    let points = this.curve.handles()
    points[points.length - 1] = vec//.toArray()
    this.curve.setHandles(points)
    this.viewport.elementChanged()
  }

  dispose() {
    if(!this.curve) return
    let points = this.curve.handles()
    points.pop()
    if(points.length < 2) {
      this.curve.remove()
      this.curve = null
      this.viewport.elementChanged()
      return
    }
    this.curve.setHandles(points)
    this.constrainPoint(this.curve, points.length - 1, this.endSnap)
    this.viewport.elementChanged()
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
    this.acceptsInput = item => this.acceptsItem(item)
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
    return item instanceof Line || item instanceof Circle || item instanceof Arc
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

export class DimensionTool extends HighlightTool {
  static icon = 'ruler'

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
