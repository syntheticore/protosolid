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
    return comp.hasAncestor(this.document.activeComponent)
  }

  loadTree(comp, recursive) {
    if(comp.hidden) return
    // Load Bodies
    const isActive = this.isActive(comp)
    const surfaceMaterial = this.getSurfaceMaterial(comp)
    comp.updateSolids()
    comp.solids.forEach(solid => {
      const mode = this.renderer.displayMode
      // Load Faces
      if(mode == 'shaded' || mode == 'wireShade') {
        const faces = solid.get_faces()
        faces.forEach(face => {
          const faceMesh = this.renderer.convertMesh(
            face.tesselate(),
            surfaceMaterial,
          )
          faceMesh.alcType = 'face'
          faceMesh.alcFace = face
          // faceMesh.alcComponent = comp
          faceMesh.alcProjectable = isActive
          faceMesh.castShadow = isActive
          faceMesh.receiveShadow = isActive
          this.renderer.add(faceMesh)
          comp.cache.faces.push(faceMesh)
          // const normal = this.convertLine(face.get_normal(), this.renderer.materials.selectionLine)
          // this.renderer.add(normal)
        })
      }
      // Load Edges
      if(mode == 'wireframe' || (isActive && mode == 'wireShade')) {
        const edges = solid.get_edges()
        const wireMaterial = this.getWireMaterial(comp)
        comp.cache.edges = (comp.cache.edges || []).concat(edges.map(edge => {
          const line = this.renderer.convertLine(edge.tesselate(), wireMaterial)
          line.alcType = 'edge'
          line.alcEdge = edge
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
    comp.cache.edges.forEach(edge => this.renderer.remove(edge))
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
    elem.mesh = line
    comp.cache.curves.push(elem)
    this.onLoadElement(elem, comp)
  }

  unloadElement(elem, comp) {
    this.renderer.remove(elem.mesh)
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

  getSurfaceMaterial(comp, highlight) {
    const material = comp.getMaterial()
    const surfaceMaterial = material ?
      material.displayMaterial :
      this.renderer.materials.surface
    return highlight ?
      this.renderer.materials.highlightSurface :
      this.isActive(comp) ?
        surfaceMaterial : this.renderer.materials.ghostSurface
  }

  getWireMaterial(comp, highlight) {
    return highlight ?
      this.renderer.materials.selectionLine :
      this.isActive(comp) ?
        this.renderer.materials.wire : this.renderer.materials.ghostWire
  }

  applyMaterials(comp, highlight, solidId) {
    const surfaceMaterial = this.getSurfaceMaterial(comp, highlight)
    const wireMaterial = this.getWireMaterial(comp, highlight)
    const faces = solidId ?
      comp.cache.faces.filter(f => f.alcFace.get_solid_id() == solidId) :
      comp.cache.faces
    const edges = solidId ?
      comp.cache.edges.filter(f => f.alcEdge.get_solid_id() == solidId) :
      comp.cache.edges
    faces.forEach(face => face.material = surfaceMaterial )
    edges.forEach(edge => edge.material = wireMaterial )
    if(!solidId) comp.children.forEach(child => this.applyMaterials(child, highlight))
  }

  highlightComponent(comp, solidId) {
    this.applyMaterials(comp, true, solidId)
  }

  unhighlightComponent(comp) {
    this.applyMaterials(comp, false)
  }

  previewFeature(comp, bufferGeometry) {
    this.renderer.remove(this.previewMesh)
    this.previewMesh = this.renderer.convertMesh(
      bufferGeometry,
      this.renderer.materials.previewAddSurface,
    )
    this.renderer.add(this.previewMesh)
    this.renderer.render()
  }

  unpreviewFeature() {
    this.renderer.remove(this.previewMesh)
    this.renderer.render()
  }
}
