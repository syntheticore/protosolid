import * as THREE from 'three'

class Tool {
  constructor(component, renderer) {
    this.component = component
    this.renderer = renderer
  }
}

export class LineTool extends Tool {
  constructor(component, renderer) {
    super(component, renderer)
    // this.line = []
    console.log('LineTool')
  }

  mouseDown(vec) {
    if(this.line) {
      let p1 = this.line.get_handles()[0]
      this.line.set_handles([p1, vec.toArray()])
    }
    this.line = this.component.add_line(vec.toArray(), vec.toArray())
    this.renderer.emitter.emit('component-changed', this.component)
    // if(vec) this.line.push(vec)
    // if(this.line.length == 2) {
    //   this.component.add_line(this.line[0].toArray(), this.line[1].toArray())
    //   this.line.length = 0
    //   this.renderer.emitter.emit('component-changed', this.component)
    // }
  }

  mouseMove(vec) {
    if(this.line) {
      let p1 = this.line.get_handles()[0]
      this.line.set_handles([p1, vec.toArray()])
    }
    this.renderer.emitter.emit('component-changed', this.component)
  }

  dispose() {
    if(!this.line) return
    const index = this.component.get_sketch_elements().length - 1
    console.log('dispose', index)
    if(index != -1) this.component.remove_element(index)
    this.renderer.emitter.emit('component-changed', this.component)
  }
}
