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
    const params = this.parent ? this.parent.getParameters() : []
    this.parameters.forEach(own => {
      const index = params.findIndex(other => other.name == own.name)
      if(index == -1) return
      params[index] = own
    })
    return params
  }
}
