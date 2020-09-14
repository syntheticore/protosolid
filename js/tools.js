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
    if(this.viewport.selectedElement) this.viewport.selectedElement.three.material = this.viewport.lineMaterial
    this.viewport.$emit('element-selected', null)
    this.viewport.transformControl.detach()
    this.viewport.render()
  }

  mouseDown(vec, coords) {
    const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
    if(!object) return
    if(this.viewport.selectedElement) this.viewport.selectedElement.three.material = this.viewport.lineMaterial
    object.material = this.viewport.selectionLineMaterial
    this.viewport.$emit('element-selected', object.element)
    this.viewport.transformControl.attach(object)
    this.viewport.render()
  }

  mouseMove(vec, coords) {
    const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
    if(!object) return this.viewport.render()
    const oldMaterial = object.material
    object.material = this.viewport.highlightLineMaterial
    this.viewport.render()
    object.material = oldMaterial
  }
}


export class SelectionTool extends ManipulationTool {
  constructor(component, viewport, callback) {
    super(component, viewport)
    this.callback = callback
  }

  click(coords) {
    const object = this.viewport.objectsAtScreen(coords, 'alcSelectable')[0]
    this.callback(object && object.element)
  }

  mouseDown() {}
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
      this.circle = null
      this.center = null
    } else {
      this.center = vec
    }
  }

  mouseMove(vec) {
    if(!this.center) return
    if(this.circle) this.component.remove_element(this.circle.id())
    const radius = vec.distanceTo(this.center)
    this.circle = this.component.add_circle(this.center.toArray(), radius)
    // this.viewport.elementChanged(this.circle, this.component)
    this.viewport.componentChanged(this.component)
  }

  dispose() {
    if(!this.spline) return
    let points = this.spline.get_handles()
    points.pop()
    this.spline.set_handles(points)
    this.viewport.elementChanged(this.spline, this.component)
  }
}


// class Component {
//   constructor(real) {
//     this.real = real
//     this.children = []
//   }

//   buildThree() {
//     const segments = node.get_sketch_elements()
//     segments.forEach(segment => {
//       const vertices = segment.default_tesselation().map(vertex => new THREE.Vector3().fromArray(vertex))
//       // const handles = segment.get_handles().map(handle => new THREE.Vector3().fromArray(handle))
//       // var lineGeom = new THREE.BufferGeometry().setFromPoints(vertices)
//       // // var pointGeom = new THREE.BufferGeometry().setFromPoints(handles)
//       // var line = new THREE.Line(lineGeom, lineMaterial)
//       // // var points = new THREE.Points(pointGeom, pointMaterial)
//       // this.scene.add(line)
//       // // this.scene.add(points)
//       var geometry = new LineGeometry()
//       geometry.setPositions(vertices.flatMap(vertex => vertex.toArray()))
//       geometry.setColors(Array(vertices.length * 3).fill(1))
//       var line = new Line2(geometry, lineMaterial)
//       line.computeLineDistances()
//       // line.scale.set(1, 1, 1)
//       line.alcSelectable = true
//       this.elements.push(line)
//       this.scene.add(line)
//     })
//   }

//   createComponent() {
//     const realChild = this.real.create_component('Untitled')
//     const comp = new Component(realChild)
//     this.children.push(comp)
//     return comp
//   }
// }
