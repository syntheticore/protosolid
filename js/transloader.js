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
    if(!comp) return
    const compId = comp.id()
    if(compId === this.document.activeComponent.id()) return true
    const compData = this.document.data[compId]
    return this.isActive(compData.parent)
  }

  loadTree(comp, recursive, isParentActive) {
    const compData = this.document.data[comp.id()]
    if(compData.hidden) return
    const isActive = this.isActive(comp)
    let solids = comp.get_solids()
    solids.forEach(solid => {
      const mode = this.renderer.displayMode
      if(mode == 'Shaded' || mode == 'Shaded + Wire') {
        const faces = solid.get_faces()
        faces.forEach(face => {
          const faceMesh = this.renderer.convertMesh(
            face.tesselate(),
            isActive ?
            this.renderer.materials.surface :
            this.renderer.materials.ghostSurface
          )
          faceMesh.alcType = 'face'
          faceMesh.alcFace = face
          faceMesh.alcComponent = comp
          faceMesh.alcProjectable = true
          faceMesh.castShadow = true
          faceMesh.receiveShadow = true
          this.renderer.add(faceMesh)
          compData.faces.push(faceMesh)
          // const normal = this.convertLine(face.get_normal(), this.renderer.materials.selectionLine)
          // this.renderer.add(normal)
        })
      }
      if(mode == 'Wireframe' || mode == 'Shaded + Wire') {
        const wireframe = solid.get_edges()
        compData.wireframe = (compData.wireframe || []).concat(wireframe.map(edge => {
          // edge = edge.map(vertex => vertex.map(dim => dim + Math.random() / 5))
          const line = this.renderer.convertLine(
            edge,
            isActive ? this.renderer.materials.wire : this.renderer.materials.ghostWire,
          )
          this.renderer.add(line)
          return line
        }))
      }
    })
    if(isActive) {
      this.updateRegions(comp)
      // Load sketch elements
      const elements = comp.get_sketch().get_sketch_elements()
      elements.forEach(element => this.loadElement(element, comp))
    }
    if(recursive) comp.get_children().forEach(child => this.loadTree(child, true, isActive))
  }

  unloadTree(comp, recursive) {
    const compData = this.document.data[comp.id()]
    compData.curves.forEach(elem => this.unloadElement(elem, comp))
    compData.wireframe.forEach(edge => this.renderer.remove(edge))
    compData.faces.forEach(faceMesh => this.renderer.remove(faceMesh))
    this.purgeRegions(compData)
    if(recursive) comp.get_children().forEach(child =>
      this.unloadTree(child, true)
    )
  }

  loadElement(elem, comp) {
    this.unloadElement(elem, comp)
    const line = this.renderer.convertLine(elem.tesselate(), this.renderer.materials.line)
    line.alcType = 'curve'
    line.alcElement = elem
    this.renderer.add(line)
    this.document.data[elem.id()] = line
    this.document.data[comp.id()].curves.push(elem)
    this.onLoadElement(elem, comp)
  }

  unloadElement(elem, comp) {
    this.renderer.remove(this.document.data[elem.id()])
    const compId = comp.id()
    const curves = this.document.data[compId].curves
    this.document.data[compId].curves = curves.filter(e => e != elem)
    this.onUnloadElement(elem, comp)
  }

  updateRegions(comp) {
    const compData = this.document.data[comp.id()]
    this.purgeRegions(compData)
    const regions = comp.get_sketch().get_regions(false)
    // console.log('# regions: ', regions.length)
    compData.regions = regions.map(region => {
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

  purgeRegions(compData) {
    compData.regions.forEach(mesh => {
      mesh.alcRegion.free()
      this.renderer.remove(mesh)
    })
    compData.regions = []
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
