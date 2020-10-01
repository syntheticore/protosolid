import * as THREE from 'three'

class Tool {
  constructor(component, viewport) {
    this.component = component
    this.viewport = viewport
  }

  click() {}

  mouseDown() {}

  mouseMove() {}

  dispose() {}
}


export class ManipulationTool extends Tool {
  click(coords) {
    const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
    if(object) return this.viewport.render()
    if(this.viewport.selectedElement) {
      this.viewport.data[this.viewport.selectedElement.id()].material = this.viewport.lineMaterial
    }
    this.viewport.$emit('element-selected', null)
    this.viewport.transformControl.detach()
    this.viewport.render()
  }

  mouseDown(vec, coords) {
    const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
    if(!object) return
    if(this.viewport.selectedElement) this.viewport.data[this.viewport.selectedElement.id()].material = this.viewport.lineMaterial
    object.material = this.viewport.selectionLineMaterial
    this.viewport.$emit('element-selected', object.element)
    this.viewport.transformControl.attach(object)
    this.viewport.render()
  }

  mouseMove(vec, coords) {
    const handle = this.viewport.activeHandle
    if(handle) {
      let handles = handle.elem.get_handles()
      handles[handle.index] = vec.toArray()
      handle.elem.set_handles(handles)
      this.viewport.elementChanged(handle.elem, this.component)
    } else {
      const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
      if(!object) return this.viewport.render()
      const oldMaterial = object.material
      object.material = this.viewport.highlightLineMaterial
      this.viewport.render()
      object.material = oldMaterial
    }
  }
}


class SelectionTool extends ManipulationTool {
  constructor(component, viewport, callback) {
    super(component, viewport)
    this.callback = callback
  }

  click(coords) {
    const selection = this.select(coords)
    if(!selection) return
    const [item, center] = selection
    this.callback(item, center)
  }
}


export class ObjectSelectionTool extends SelectionTool {
  select(coords) {
    const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
    return object && [object.element, new THREE.Vector3().fromArray(object.element.get_handles()[0])]
  }
}


export class ProfileSelectionTool extends SelectionTool {
  select(coords) {
    // const regions = this.component.get_regions()
    // console.log('Regions', regions)
    const splits = this.component.get_all_split()
    console.log('ProfileSelectionTool', splits)
    // const elems = this.component.get_sketch_elements()
    // // .map(elem => elem.get_handles())
    // elems.forEach(elem => {
    //   this.component.remove_element(elem.id())
    // })
    this.viewport.componentChanged(this.component)
  }
}


export class LineTool extends Tool {
  mouseDown(vec) {
    this.mouseMove(vec)
    this.line = this.component.add_line(vec.toArray(), vec.toArray())
    this.viewport.elementChanged(this.line, this.component)
  }

  mouseMove(vec) {
    if(!this.line) return
    let p1 = this.line.get_handles()[0]
    this.line.set_handles([p1, vec.toArray()])
    this.viewport.elementChanged(this.line, this.component)
  }

  dispose() {
    if(!this.line) return
    this.component.remove_element(this.line.id())
    this.viewport.componentChanged(this.component)
  }
}


export class SplineTool extends Tool {
  mouseDown(vec) {
    if(this.spline) {
      let points = this.spline.get_handles()
      points[points.length - 1] = vec.toArray()
      points.push(vec.toArray())
      this.spline.set_handles(points)
    } else {
      this.spline = this.component.add_spline([vec.toArray(), vec.toArray()])
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
  mouseDown(vec) {
    if(this.center) {
      this.center = null
      this.circle = null
    } else {
      this.center = vec
      this.circle = this.component.add_circle(vec.toArray(), 1)
    }
  }

  mouseMove(vec) {
    if(!this.center) return
    this.circle.set_handles([this.center.toArray(), vec.toArray()])
    this.viewport.elementChanged(this.circle, this.component)
  }
}
