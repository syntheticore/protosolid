import * as THREE from 'three'

import { vec2three } from './utils.js'

class Tool {
  constructor(component, viewport) {
    this.component = component
    this.viewport = viewport
    this.enableSnapping = false
  }

  click() {}

  mouseDown() {}

  mouseUp() {}

  mouseMove() {}

  dispose() {}
}


class HighlightTool extends Tool {
  constructor(component, viewport, selectors) {
    super(component, viewport)
    this.selectors = selectors
  }

  mouseMove(vec, coords) {
    if(this.viewport.hoveredHandle) return this.viewport.renderer.render()
    const object = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    if(!object) return this.viewport.renderer.render()
    const oldMaterial = object.material
    object.material = {
      curve: this.viewport.renderer.materials.highlightLine,
      region: this.viewport.renderer.materials.highlightRegion,
      face: this.viewport.renderer.materials.highlightSurface,
    }[object.alcType]
    this.viewport.renderer.render()
    object.material = oldMaterial
  }
}


export class ManipulationTool extends HighlightTool {
  constructor(component, viewport) {
    super(component, viewport, ['curve'])
  }

  click(vec, coords) {
    const curve = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    if(curve) return this.viewport.renderer.render()
    const type = this.viewport.selection && this.viewport.selection.typename()
    if(this.viewport.selection && type !== 'Solid' && type !== 'Component') {
      console.log(this.viewport.selection)
      this.viewport.selection.mesh.material = this.viewport.renderer.materials.line
    }
    this.viewport.$emit('update:selection', null)
    // this.viewport.renderer.removeGizmo()
  }

  mouseDown(vec, coords) {
    if(this.viewport.activeHandle) {
      this.enableSnapping = true
      return
    }
    const curve = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    if(!curve) return
    const type = this.viewport.selection && this.viewport.selection.typename()
    if(this.viewport.selection && type !== 'Solid' && type !== 'Component') {
      this.viewport.selection.mesh.material = this.viewport.renderer.materials.line
    }
    curve.material = this.viewport.renderer.materials.selectionLine
    this.viewport.$emit('update:selection', curve.alcElement)
    // this.viewport.gizmo.attach(object)
  }

  mouseUp(vec, coords) {
    this.enableSnapping = false
  }

  mouseMove(vec, coords) {
    const handle = this.viewport.activeHandle
    if(handle) {
      let handles = handle.elem.get_handles()
      handles[handle.index] = vec.toArray()
      handle.elem.set_handles(handles)
      this.viewport.elementChanged(handle.elem, this.component)
    } else {
      super.mouseMove(vec, coords)
    }
  }
}


export class PlaneTool extends HighlightTool {
  constructor(component, viewport) {
    super(component, viewport, ['face'])
  }

  click(vec, coords) {
    const face = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    if(face && face.alcFace.get_surface_type() == 'Planar') {
      this.viewport.renderer.sketchPlane.position = vec2three(face.alcFace.get_origin())
      this.viewport.renderer.sketchPlane.setNormal(vec2three(face.alcFace.get_normal()))
    }
    this.viewport.renderer.render()
  }

  mouseDown(vec, coords) {
    const face = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    this.viewport.renderer.render()
  }
}


export class TrimTool extends Tool {
  click(vec, coords) {
    const curve = this.viewport.renderer.objectsAtScreen(coords, 'curve')[0]
    if(curve) return this.viewport.renderer.render()

    this.viewport.renderer.render()
  }

  mouseDown(vec, coords) {
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


class SelectionTool extends HighlightTool {
  constructor(component, viewport, selectors, callback) {
    super(component, viewport, selectors)
    this.callback = callback
  }

  mouseDown(vec, coords) {
    const mesh = this.viewport.renderer.objectsAtScreen(coords, this.selectors)[0]
    if(!mesh) return
    const selection = this.select(mesh)
    this.callback(selection, mesh)
  }
}


export class ObjectSelectionTool extends SelectionTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['curve'], callback)
  }

  select(mesh) {
    return mesh.alcElement
  }
}


export class ProfileSelectionTool extends SelectionTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['region'], callback)
  }

  select(mesh) {
    return mesh.alcRegion
  }
}


export class FaceSelectionTool extends SelectionTool {
  constructor(component, viewport, callback) {
    super(component, viewport, ['face'], callback)
  }

  select(mesh) {
    return mesh.alcFaceId
  }
}


export class LineTool extends Tool {
  constructor(component, viewport) {
    super(component, viewport)
    this.enableSnapping = true
  }

  mouseDown(vec) {
    this.mouseMove(vec)
    const sketch = this.component.real.get_sketch()
    const elems = sketch.get_sketch_elements()
    elems.pop()
    const touchesExisting = elems
      .flatMap(elem => elem.get_snap_points() )
      .map(p => vec2three(p) )
      .some(p => p.equals(vec) )
    // Restart tool when we hit an existing point
    if(touchesExisting && this.line) {
      this.line = null
    } else {
      this.line = sketch.add_line(vec.toArray(), vec.toArray())
      this.viewport.elementChanged(this.line, this.component)
    }
  }

  mouseMove(vec) {
    if(!this.line) return
    let p1 = this.line.get_handles()[0]
    this.line.set_handles([p1, vec.toArray()])
    this.viewport.elementChanged(this.line, this.component)
  }

  dispose() {
    if(!this.line) return
    this.component.real.get_sketch().remove_element(this.line.id())
    this.viewport.componentChanged(this.component)
  }
}


export class SplineTool extends Tool {
  constructor(component, viewport) {
    super(component, viewport)
    this.enableSnapping = true
  }

  mouseDown(vec) {
    if(this.spline) {
      let points = this.spline.get_handles()
      points[points.length - 1] = vec.toArray()
      points.push(vec.toArray())
      this.spline.set_handles(points)
    } else {
      this.spline = this.component.real.get_sketch().add_spline([vec.toArray(), vec.toArray()])
    }
    this.viewport.elementChanged(this.spline, this.component)
  }

  mouseMove(vec) {
    if(!this.spline) return
    let points = this.spline.get_handles()
    points[points.length - 1] = vec.toArray()
    this.spline.set_handles(points)
    this.viewport.elementChanged(this.spline, this.component)
  }

  dispose() {
    if(!this.spline) return
    let points = this.spline.get_handles()
    points.pop()
    this.spline.set_handles(points)
    this.viewport.elementChanged(this.spline, this.component)
  }
}


export class CircleTool extends Tool {
  constructor(component, viewport) {
    super(component, viewport)
    this.enableSnapping = true
  }

  mouseDown(vec) {
    if(this.center) {
      this.center = null
      this.circle = null
    } else {
      this.center = vec
      this.circle = this.component.real.get_sketch().add_circle(vec.toArray(), 1)
    }
  }

  mouseMove(vec) {
    if(!this.center) return
    this.circle.set_handles([this.center.toArray(), vec.toArray()])
    this.viewport.elementChanged(this.circle, this.component)
  }
}


export class ArcTool extends Tool {
  constructor(component, viewport) {
    super(component, viewport)
    this.enableSnapping = true
  }

  mouseDown(vec) {
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
    this.arc = this.arc || this.component.real.get_sketch().add_arc(
      this.start.toArray(),
      vec.toArray(),
      this.end.toArray()
    )
    this.arc.set_initial_handles([this.start.toArray(), vec.toArray(), this.end.toArray()])
    this.viewport.elementChanged(this.arc, this.component)
  }
}
