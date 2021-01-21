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
    this.solids = []

    this.cache = {
      faces: [],
      edges: [],
      regions: [],
      curves: [],
    }
  }

  typename() {
    return 'Component'
  }

  createComponent(title) {
     let comp = this.real.create_component()
     comp = new Component(comp, this, title || 'New Component')
     this.children.push(comp)
     return comp
  }

  deleteComponent(comp) {
    this.children = this.children.filter(child => child !== comp )
    this.real.delete_component(comp.real)
  }

  getMaterial() {
    return this.material || (this.parent && this.parent.getMaterial())
  }

  // Returns zero for empty components,
  // but undefined when weight could not be determined
  getWeight() {
    if(this.solids.length && !this.material) return
    try {
      let weight = this.children.reduce((acc, child) => {
        const childWeight = child.getWeight()
        if(childWeight === undefined) throw 'no weight'
        return acc + childWeight
      }, 0.0)
      return weight + (this.material ? this.getVolume() * this.material.density : 0.0)
    } catch(e) {}
  }

  getVolume() {
    return this.solids.reduce((acc, solid) => acc + solid.volume, 0.0)
  }

  updateSolids() {
    this.solids = this.real.get_solids()
    this.solids.forEach(solid => solid.component = this )
  }

  hasAncestor(parent) {
    if(this === parent) return true
    return this.parent && this.parent.hasAncestor(parent)
  }

  getParameters() {
    const params = [...this.parameters]
    const parentParams = this.parent ? this.parent.getParameters() : []
    parentParams.forEach(other => {
      const index = params.findIndex(own => own.name == other.name)
      if(index == -1) params.push(other)
    })
    return params
  }

  // isHidden() {
  //   return this.hidden || (this.parent && this.parent.isHidden())
  // }

  free() {
    this.children.forEach(child => child.free() )
    this.real.free()
  }
}
