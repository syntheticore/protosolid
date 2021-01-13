export default class Component {
  constructor(realComponent, parent, title) {
    this.real = realComponent
    this.parent = parent
    this.title = title

    this.hidden = false
    this.material = null
    this.cog = false
    this.sectionViews = []
    this.parameters = []

    this.children = []

    this.cache = {
      faces: [],
      wireframe: [],
      regions: [],
      curves: [],
    }
  }

  createComponent(title) {
     let comp = this.real.create_component()
     comp = new Component(comp, this, title || 'New Component')
     this.children.push(comp)
     return comp
  }

  getParameters() {

  }
}
