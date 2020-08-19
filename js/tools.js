import * as THREE from 'three'

class Tool {
  constructor(component, viewport) {
    this.component = component
    this.viewport = viewport
  }
}

export class LineTool extends Tool {
  mouseDown(vec) {
    this.mouseMove(vec)
    this.line = this.component.add_line(vec.toArray(), vec.toArray())
    this.viewport.componentChanged(this.component)
  }

  mouseMove(vec) {
    if(!this.line) return
    let p1 = this.line.get_handles()[0]
    this.line.set_handles([p1, vec.toArray()])
    this.viewport.componentChanged(this.component)
  }

  dispose() {
    if(!this.line) return
    const index = this.component.get_sketch_elements().length - 1
    console.log('dispose', index)
    if(index != -1) this.component.remove_element(index)
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
    this.viewport.componentChanged(this.component)
  }

  mouseMove(vec) {
    if(!this.spline) return
    let points = this.spline.get_handles()
    points[points.length - 1] = vec.toArray()
    this.spline.set_handles(points)
    this.viewport.componentChanged(this.component)
  }

  dispose() {
    if(!this.spline) return
    let points = this.spline.get_handles()
    points.pop()
    this.spline.set_handles(points)
    this.viewport.componentChanged(this.component)
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
