import * as THREE from 'three'

export class Transloader {
  constructor(renderer, onLoadElement, onUnloadElement) {
    this.renderer = renderer
    this.onLoadElement = onLoadElement
    this.onUnloadElement = onUnloadElement
  }

  setDocument(doc) {
    this.document = doc
  }

  isActive(comp) {
    return this.hasParent(comp, this.document.activeComponent)
  }

  isHighlighted(comp) {
    if(!this.highlightComponent) return
    return this.hasParent(comp, this.highlightComponent)
  }

  hasParent(comp, parent) {
    if(!comp) return
    if(comp === parent) return true
    return this.hasParent(comp.parent, parent)
  }

  loadTree(comp, recursive) {
    if(comp.hidden) return
    const isActive = this.isActive(comp)
    const isHighlighted = this.isHighlighted(comp)
    // Load Bodies
    let solids = comp.real.get_solids()
    solids.forEach(solid => {
      const mode = this.renderer.displayMode
      // Load Faces
      if(mode == 'shaded' || mode == 'wireShade') {
        const faces = solid.get_faces()
        faces.forEach(face => {
          const faceMesh = this.renderer.convertMesh(
            face.tesselate(),
            isActive ?
            isHighlighted ?
            this.renderer.materials.highlightSurface :
            this.renderer.materials.surface :
            isHighlighted ?
            this.renderer.materials.previewAddSurface :
            this.renderer.materials.ghostSurface
          )
          faceMesh.alcType = 'face'
          faceMesh.alcFace = face
          faceMesh.alcComponent = comp
          faceMesh.alcProjectable = true
          faceMesh.castShadow = true
          faceMesh.receiveShadow = true
          this.renderer.add(faceMesh)
          comp.cache.faces.push(faceMesh)
          // const normal = this.convertLine(face.get_normal(), this.renderer.materials.selectionLine)
          // this.renderer.add(normal)
        })
      }
      // Load Edges
      if(mode == 'wireframe' || (isActive && mode == 'wireShade')) {
        const wireframe = solid.get_edges()
        comp.cache.wireframe = (comp.cache.wireframe || []).concat(wireframe.map(edge => {
          // edge = edge.map(vertex => vertex.map(dim => dim + Math.random() / 5))
          const line = this.renderer.convertLine(
            edge,
            isHighlighted ?
            this.renderer.materials.selectionLine :
            isActive ?
            this.renderer.materials.wire : this.renderer.materials.ghostWire,
          )
          this.renderer.add(line)
          return line
        }))
      }
    })
    // Load Sketch Elements
    if(comp === this.document.activeComponent) {
      const elements = comp.real.get_sketch().get_sketch_elements()
      elements.forEach(element => this.loadElement(element, comp))
      if(!comp.cache.regions.length) this.updateRegions(comp)
    }
    // Recurse
    if(recursive) comp.children.forEach(child => this.loadTree(child, true))
  }

  unloadTree(comp, recursive) {
    comp.cache.curves.forEach(elem => this.unloadElement(elem, comp))
    comp.cache.wireframe.forEach(edge => this.renderer.remove(edge))
    comp.cache.faces.forEach(faceMesh => this.renderer.remove(faceMesh))
    this.purgeRegions(comp)
    if(recursive) comp.children.forEach(child =>
      this.unloadTree(child, true)
    )
  }

  loadElement(elem, comp) {
    this.unloadElement(elem, comp)
    const line = this.renderer.convertLine(elem.tesselate(), this.renderer.materials.line)
    line.alcType = 'curve'
    line.alcElement = elem
    this.renderer.add(line)
    elem.line = line
    comp.cache.curves.push(elem)
    this.onLoadElement(elem, comp)
  }

  unloadElement(elem, comp) {
    this.renderer.remove(elem.line)
    comp.cache.curves = comp.cache.curves.filter(e => e != elem)
    this.onUnloadElement(elem, comp)
  }

  updateRegions(comp) {
    this.purgeRegions(comp)
    const regions = comp.real.get_sketch().get_regions(false)
    // console.log('# regions: ', regions.length)
    comp.cache.regions = regions.map(region => {
      // let material = this.renderer.materials.region.clone()
      // material.color = new THREE.Color(Math.random(), Math.random(), Math.random())
      const mesh = this.renderer.convertMesh(
        region.get_mesh(),
        this.renderer.materials.region
      )
      mesh.alcType = 'region'
      mesh.alcRegion = region
      this.renderer.add(mesh)
      return mesh
    })
  }

  purgeRegions(comp) {
    comp.cache.regions.forEach(mesh => {
      if(mesh.alcRegion.noFree) {
        mesh.alcRegion.unused = true
      } else {
        mesh.alcRegion.free()
      }
      this.renderer.remove(mesh)
    })
    comp.cache.regions = []
  }

  previewFeature(comp, bufferGeometry) {
    this.renderer.remove(this.previewMesh)
    this.previewMesh = this.renderer.convertMesh(
      bufferGeometry,
      this.renderer.materials.previewAddSurface
    );
    this.renderer.add(this.previewMesh)
    this.renderer.render()
  }

  unpreviewFeature() {
    this.renderer.remove(this.previewMesh)
    this.renderer.render()
  }
}
