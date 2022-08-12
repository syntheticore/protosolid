import * as THREE from 'three'

import {
  vec2three,
  matrix2three,
  matrixFromThree,
  rotationFromNormal
} from './utils.js'

class Tool {
  constructor(component, viewport) {
    this.component = component
    this.viewport = viewport
    this.enableSnapping = false
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
  }

  mouseMove(vec, coords) {}

  dispose() {}
}


export class DummyTool extends Tool {}


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
    if(this.viewport.hoveredHandle) return this.viewport.renderer.render()
    if(this.viewport.pickingPath) this.viewport.updatePath(this.viewport.pickingPath)
    const object = await this.getObject(coords, true)
    this.viewport.$emit('update:highlight', object)
  }

  getObject(coords, any) {
    return new Promise(resolve => {
      let items = this.viewport.renderer
        .objectsAtScreen(coords, this.realSelectors)
        .map(obj => (obj.alcType == 'face' && this.selectors.some(s => s == 'solid' )) ? obj.alcObject.solid : obj.alcObject )
        .filter(obj => this.viewport.transloader.isActive(obj) )
      items = Array.from(new Set(items))
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
    this.setSelectors(this.viewport.activeSketch ? ['curve'] : ['curve', 'solid'])
  }

  async click(vec, coords) {
    const curve = await this.getObject(coords)
    if(curve) {
      this.viewport.$emit('update:selection', this.viewport.selection.handle(curve, this.viewport.$root.isCtrlPressed))
    } else {
      if(this.viewport.$root.isCtrlPressed) return this.viewport.renderer.render()
      this.viewport.$emit('update:selection', this.viewport.selection.clear())
    }
  }

  async mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.viewport.activeHandle) this.enableSnapping = true
  }

  mouseUp(vec, coords) {
    super.mouseUp(vec, coords)
    this.enableSnapping = false
  }

  mouseMove(vec, coords) {
    const handle = this.viewport.activeHandle
    if(handle) {
      let handles = handle.elem.handles()
      handles[handle.index] = vec.toArray()
      handle.elem.set_handles(handles, false)
      this.viewport.elementChanged(handle.elem, this.component)
    } else {
      super.mouseMove(vec, coords)
    }
  }
}


// export class PlaneTool extends HighlightTool {
//   constructor(component, viewport) {
//     super(component, viewport, ['face'])
//   }

//   click(vec, coords) {
//     const face = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
//     if(face && face.alcObject.surface_type() == 'Planar') {
//       const position = vec2three(face.alcObject.origin())
//       let rotation = rotationFromNormal(vec2three(face.alcObject.normal()))

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


export class ObjectPickTool extends PickTool {
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

export class PlanePickTool extends PickTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['plane', 'face'], callback)
  }
}


export class SketchTool extends Tool {
  constructor(component, viewport, sketch) {
    super(component, viewport)
    this.sketch = sketch
    this.enableSnapping = true
  }
}


export class LineTool extends SketchTool {
  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    this.mouseMove(vec)
    const elems = this.sketch.sketch_elements()
    elems.pop()
    const touchesExisting = elems
      .flatMap(elem => elem.snap_points() )
      .map(p => vec2three(p) )
      .some(p => p.equals(vec) )
    // Restart tool when we hit an existing point
    if(touchesExisting && this.line) {
      this.line = null
    } else {
      this.line = this.sketch.add_line(vec.toArray(), vec.toArray())
      this.viewport.elementChanged(this.line, this.component)
    }
  }

  mouseMove(vec) {
    if(!this.line) return
    let p1 = this.line.handles()[0]
    this.line.set_handles([p1, vec.toArray()], false)
    this.viewport.elementChanged(this.line, this.component)
  }

  dispose() {
    if(!this.line) return
    this.line.remove()
    this.viewport.componentChanged(this.component)
  }
}


export class SplineTool extends SketchTool {
  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.spline) {
      let points = this.spline.handles()
      points[points.length - 1] = vec.toArray()
      points.push(vec.toArray())
      this.spline.set_handles(points, false)
    } else {
      this.spline = this.sketch.add_spline([vec.toArray(), vec.toArray()])
    }
    this.viewport.elementChanged(this.spline, this.component)
  }

  mouseMove(vec) {
    if(!this.spline) return
    let points = this.spline.handles()
    points[points.length - 1] = vec.toArray()
    this.spline.set_handles(points, false)
    this.viewport.elementChanged(this.spline, this.component)
  }

  dispose() {
    if(!this.spline) return
    let points = this.spline.handles()
    points.pop()
    this.spline.set_handles(points, false)
    this.viewport.elementChanged(this.spline, this.component)
  }
}


export class CircleTool extends SketchTool {
  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.center) {
      this.center = null
      this.circle = null
    } else {
      this.center = vec
      this.circle = this.sketch.add_circle(vec.toArray(), 1)
    }
  }

  mouseMove(vec) {
    if(!this.center) return
    this.circle.set_handles([this.center.toArray(), vec.toArray()], false)
    this.viewport.elementChanged(this.circle, this.component)
  }
}


export class ArcTool extends SketchTool {
  constructor(component, viewport, sketch) {
    super(component, viewport, sketch)
  }

  mouseDown(vec, coords) {
    super.mouseDown(vec, coords)
    if(this.start && this.end) {
      this.start = null
      this.end = null
      this.arc = null
    } else if(this.start) {
      this.end = vec
    } else {
      this.start = vec
    }
  }

  mouseMove(vec) {
    if(!this.start || !this.end) return
    // #add_arc can fail for for colinear inputs
    try {
      this.arc = this.arc || this.sketch.add_arc(
        this.start.toArray(),
        vec.toArray(),
        this.end.toArray()
      )
      this.arc.set_handles([this.start.toArray(), vec.toArray(), this.end.toArray()], true)
      this.viewport.elementChanged(this.arc, this.component)
    } catch(e) {}
  }
}
