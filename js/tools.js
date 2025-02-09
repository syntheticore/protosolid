// import * as THREE from 'three'

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
} from './core/geom2d.js'

import {
  CoincidentConstraint,
  PerpendicularConstraint,
  HorVertConstraint,
  ParallelConstraint,
  EqualConstraint,
  TangentConstraint,
  FixConstraint,
  Dimension,
  Projection,
  ElemRef,
} from './core/sketch.js'


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
      coords.x != this.lastCoords.x ||
      coords.y != this.lastCoords.y) return this.viewport.renderer.render()
    this.click(vec, coords)
    // window.gc && window.gc()
  }

  mouseMove(vec, coords) {}

  dispose() {}
}


export class DummyTool extends Tool {
  mouseMove(vec, coords) {
    // this.viewport.renderer.render()
  }

  async click(vec, coords) {
    this.viewport.document.selection.clear()
  }
}


class HighlightTool extends Tool {
  constructor(component, viewport, selectors) {
    super(component, viewport)
    this.setSelectors(selectors)
  }

  setSelectors(selectors) {
    this.selectors = selectors
    this.realSelectors = selectors.map(selector => selector == 'solid' ? 'face' : selector )
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
      // console.log('getObject')
      let items = this.viewport.renderer
        .objectsAtScreen(coords, this.realSelectors)
        .map(obj =>
          obj.alcTypes.some(t => t == 'face' ) && this.selectors.some(s => s == 'solid' ) ?
            obj.alcObject.solid
            :
            obj.alcObject
        )
        // .map(obj => {console.log(obj); return obj})
        .filter(obj => this.viewport.transloader.isActive(obj) )
      items = Array.from(new Set(items))
      const handle = this.viewport.hoveredHandle
      if(handle && this.realSelectors.includes('point')) {
        items.unshift(new ElemRef(handle.elem, handle.index))
      }
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
    this.setSelectors(this.viewport.document.activeSketch ? ['curve'] : ['curve', 'solid'])
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
    if(!this.viewport.activeHandle && !this.viewport.activeDimension) return
    this.snapToPoints = true
    this.cursor = 'grabbing'
  }

  mouseUp(vec, coords) {
    // this.mouseMove(vec, coords)
    super.mouseUp(vec, coords)
    this.snapToPoints = false
    this.cursor = 'auto'
    const sketch = this.viewport.document.activeSketch
    const handle = this.viewport.activeHandle
    if(sketch && handle) {
      sketch.elements.forEach(elem => {
        if(elem == handle.elem) return
        const idx = elem.endpoints().findIndex(p => p.almost(vec) )
        if(idx != -1) sketch.addConstraint(
          new CoincidentConstraint(new ElemRef(handle.elem, handle.index), new ElemRef(elem, idx))
        )
      })
    }
  }

  mouseMove(vec, coords) {
    const handle = this.viewport.activeHandle
    const dimension = this.viewport.activeDimension
    if(handle) {
      let handles = handle.elem.handles()
      handles[handle.index] = vec//.toArray()
      handle.elem.setHandles(handles, false)
      this.viewport.updateSketch(true)
      // this.viewport.elementChanged(handle.elem, this.component)
    } else if(dimension) {
      dimension.position = vec
      this.viewport.updateSketch()
    } else {
      super.mouseMove(vec, coords)
    }
  }
}


export class ProjectTool extends HighlightTool {
  static icon = 'layer-group'

  constructor(component, viewport, sketch) {
    super(component, viewport, ['point', 'axis', 'edge'])
    this.sketch = sketch
    this.localSpace = true
    this.cursor = 'copy'
  }

  async click(vec, coords) {
    const edge = await this.getObject(coords)
    if(!edge) return
    const projection = new Projection(edge)
    this.sketch.addProjection(projection)
    this.viewport.updateRegions(true)
    this.viewport.document.emit('component-changed', this.component)
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

  click(vec, coords) {
    const curve = this.viewport.renderer.objectsAtScreen(coords, 'curve')[0]
    if(curve) return this.viewport.renderer.render()

    this.viewport.renderer.render()
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const curve = this.viewport.renderer.objectsAtScreen(coords, 'curve')[0]
    if(!curve) return this.viewport.renderer.render()

    this.viewport.renderer.render()
  }

  mouseMove(vec, coords) {
    const curve = this.viewport.renderer.objectsAtScreen(coords, 'curve')[0]
    if(!curve) return this.viewport.renderer.render()

    this.viewport.renderer.render()
  }
}


class PickTool extends HighlightTool {
  constructor(component, viewport, selectors, callback) {
    super(component, viewport, selectors)
    this.callback = callback
    this.localSpace = false
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    const mesh = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    if(!mesh) return
    const selection = this.select(mesh)
    this.callback(selection, mesh)
  }

  select(mesh) {
    return mesh.alcObject
  }
}


export class CurvePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['curve'], callback)
  }
}

export class ProfilePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['region'], callback)
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
}


export class LineTool extends SketchTool {
  static icon = 'pen'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    this.mouseMove(vec)

    const elems = [...this.sketch.elements]
    if(this.curve) elems.pop()
    const touchesExisting = elems.find(elem => elem.endpoints().some(p => p.equals(vec) ) )
    const endpoints = touchesExisting && touchesExisting.endpoints()
    const index = touchesExisting && endpoints.indexOf(endpoints.find(sp => sp.equals(vec) ))

    // Restart tool when we hit an existing point
    if(touchesExisting && this.curve) {
      this.sketch.add(this.curve)
      if(index != -1) this.sketch.addConstraint(
        new CoincidentConstraint(new ElemRef(this.curve, 1), new ElemRef(touchesExisting, index))
      )
      this.curve = null
    } else {
      const old = this.curve
      this.curve = new Line(vec, vec)
      this.sketch.add(this.curve)
      const other = touchesExisting || old
      const otherIndex = touchesExisting ? index : 1
      if(old || touchesExisting) this.sketch.addConstraint(
        new CoincidentConstraint(new ElemRef(this.curve, 0), new ElemRef(other, otherIndex))
      )
      if(old) {
        const ySnapped = old.endpoints()[0].x.almost(vec.x)
        const xSnapped = old.endpoints()[0].y.almost(vec.y)
        if(xSnapped || ySnapped) {
          // const type = (xSnapped ? HorizontalConstraint : VerticalConstraint)
          const type = HorVertConstraint
          this.sketch.addConstraint(new type(old))
        }
      }
    }
  }

  mouseMove(vec) {
    if(!this.curve) return
    let p1 = this.curve.endpoints()[0]
    this.curve.setHandles([p1, vec], false)
    this.viewport.elementChanged(this.curve, this.component)
  }

  dispose() {
    if(!this.curve) return
    this.curve.remove()
    this.viewport.componentChanged(this.component)
  }
}


export class SplineTool extends SketchTool {
  static icon = 'route'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.curve) {
      let points = this.curve.handles()
      points[points.length - 1] = vec//.toArray()
      // points.push(vec.toArray())
      points.push(vec)
      this.curve.setHandles(points)
    } else {
      this.curve = new Spline([vec, vec])
      this.sketch.add(this.curve)
      // this.curve = this.sketch.add_spline([vec.toArray(), vec.toArray()])
      // this.curve.sketch = this.sketch
    }
    this.viewport.elementChanged(this.curve, this.component)
  }

  mouseMove(vec) {
    if(!this.curve) return
    let points = this.curve.handles()
    points[points.length - 1] = vec//.toArray()
    this.curve.setHandles(points)
    this.viewport.elementChanged(this.curve, this.component)
  }

  dispose() {
    if(!this.curve) return
    let points = this.curve.handles()
    points.pop()
    this.curve.setHandles(points)
    this.viewport.elementChanged(this.curve, this.component)
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
      // this.curve.sketch = this.sketch
    }
  }

  mouseMove(vec) {
    if(!this.center) return
    // this.curve.setHandles([this.center.toArray(), vec.toArray()], false)
    this.curve.setHandles([this.center, vec], false)
    this.viewport.elementChanged(this.curve, this.component)
  }
}


export class ArcTool extends SketchTool {
  static icon = 'bezier-curve'

  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.start && this.end) {
      this.start = null
      this.end = null
      this.curve = null
    } else if(this.start) {
      this.end = vec
    } else {
      this.start = vec
    }
  }

  mouseMove(vec) {
    if(!this.start || !this.end) return
    if(!this.curve) {
      this.curve = Arc.fromPoints([this.start, vec, this.end])
      if(!this.curve) return
      this.sketch.add(this.curve)
    }
    this.curve.setPoints([this.start, vec, this.end], true)
    this.viewport.elementChanged(this.curve, this.component)
  }
}


export class ConstraintTool extends HighlightTool {
  constructor(component, viewport, sketch) {
    super(component, viewport, [])
    this.setSelectors(this.constructor.selectors)
    this.sketch = sketch
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
    console.log(curve)
    if(this.items.includes(curve)) return
    this.items.push(curve)
    const counts = {}
    this.selectors.forEach(sel => {
      counts[sel] = this.items.filter(item => (item.mesh ? item.mesh().alcTypes : ['point']).some(t => t == sel ) ).length
    })
    console.log(counts, this.isComplete(counts))
    if(this.isComplete(counts)) {
      const constraint = new this.constructor.constraintType(...this.items)
      this.sketch.addConstraint(constraint)
      this.viewport.updateRegions(true)
      this.items = []
    }
  }

  isComplete(counts) {}
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
}

export class TangentConstraintTool extends ConstraintTool {
  static constraintType = TangentConstraint
  static icon = 'bezier-curve'
  static selectors = ['curve']

  isComplete(counts) { return counts.curve == 2 }
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

    const done = (
      this.items.length == 2 ||
      this.items[0] instanceof Circle ||
      this.items[0] instanceof Arc
    )

    if(!done) {
      const curve = await this.getObject(coords)
      // if(!curve || !(curve instanceof Line)) {
      //   this.items = []
      //   return
      // }
      if(!curve) return
      this.items.push(curve)
    } else {
      const toSketch = this.sketch.workplane.clone().invert()
      const position = this.viewport.renderer.fromScreen(coords).applyMatrix4(toSketch)
      const constraint = new Dimension(this.items, position)
      this.sketch.addConstraint(constraint)
      this.viewport.updateRegions(true)
      this.items = []
    }
  }
}
