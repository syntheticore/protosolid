import * as THREE from 'three'

import Component from './component.js'

export default class Transloader {
  constructor(renderer, onLoadElement, onUnloadElement) {
    this.renderer = renderer
    this.onLoadElement = onLoadElement
    this.onUnloadElement = onUnloadElement
  }

  setActiveComponent(comp) {
    this.activeComponent = comp
  }

  setSelection(obj) {
    const old = this.selection
    this.selection = obj
    this.applyMaterials(old)
    this.applyMaterials(obj)
  }

  setHighlight(obj) {
    const old = this.highlight
    this.highlight = obj
    this.applyMaterials(old)
    this.applyMaterials(obj)
  }

  isActive(comp) {
    return comp.hasAncestor(this.activeComponent)
  }

  isSelected(obj) {
    return this.hasAncestor(obj, this.selection)
  }

  isHighlighted(obj) {
    return this.hasAncestor(obj, this.highlight)
  }

  hasAncestor(obj, ancestor) {
    if(obj === ancestor) return true
    if(obj.constructor === alcWasm.JsFace) return this.hasAncestor(obj.solid, ancestor)
    const comp = this.getComponent(obj)
    return comp.hasAncestor(ancestor)
  }

  getComponent(obj) {
    switch(obj.constructor) {
      case Component:
        return obj

      case alcWasm.JsSolid:
      case alcWasm.JsRegion:
      case alcWasm.JsCurve:
        return obj.component

      case alcWasm.JsFace:
        return obj.solid.component
    }
  }

  loadTree(comp, recursive) {
    if(comp.hidden) return
    // Load Bodies
    const isActive = this.isActive(comp)
    comp.updateSolids()
    const cache = comp.cache()
    comp.solids.forEach(solid => {
      const mode = this.renderer.displayMode
      // Load Faces
      if(mode == 'shaded' || mode == 'wireShade') {
        const faces = solid.get_faces()
        faces.forEach(face => {
          face.solid = solid
          const faceMesh = this.renderer.convertMesh(
            face.tesselate(),
            this.getSurfaceMaterial(comp, face),
          )
          face.mesh = faceMesh
          faceMesh.alcType = 'face'
          faceMesh.alcObject = face
          faceMesh.alcProjectable = isActive
          faceMesh.castShadow = isActive
          faceMesh.receiveShadow = isActive
          this.renderer.add(faceMesh)
          cache.faces.push(face)
          // const normal = this.convertLine(face.get_display_normal(), this.renderer.materials.selectionLine)
          // this.renderer.add(normal)
        })
      }
      // Load Edges
      if(mode == 'wireframe' || (isActive && mode == 'wireShade')) {
        const edges = solid.get_edges()
        const wireMaterial = this.getWireMaterial(comp, solid)
        cache.edges = (cache.edges || []).concat(edges.map(edge => {
          const line = this.renderer.convertLine(edge.tesselate(), wireMaterial)
          line.alcType = 'edge'
          line.alcObject = edge
          edge.mesh = line
          this.renderer.add(line)
          return edge
        }))
      }
    })
    // Load Sketch Elements
    if(comp === this.activeComponent) {
      const elements = comp.real.get_sketch().get_sketch_elements()
      elements.forEach(element => this.loadElement(element, comp))
      if(!cache.regions.length) this.updateRegions(comp)
    }
    // Recurse
    if(recursive) comp.children.forEach(child => this.loadTree(child, true))
  }

  unloadTree(comp, recursive) {
    const cache = comp.cache()
    cache.curves.forEach(elem => this.unloadElement(elem, comp))
    cache.edges.forEach(edge => {
      this.renderer.remove(edge.mesh)
      edge.free()
    })
    cache.faces.forEach(face => {
      this.renderer.remove(face.mesh)
      face.free()
    })
    cache.edges = []
    cache.faces = []
    this.purgeRegions(comp)
    if(recursive) comp.children.forEach(child =>
      this.unloadTree(child, true)
    )
  }

  loadElement(elem, comp) {
    this.unloadElement(elem, comp)
    const line = this.renderer.convertLine(elem.tesselate(), this.renderer.materials.line)
    line.alcType = 'curve'
    line.alcObject = elem
    elem.mesh = line
    elem.component = comp
    this.renderer.add(line)
    comp.cache().curves.push(elem)
    this.onLoadElement(elem, comp)
  }

  unloadElement(elem, comp) {
    const cache = comp.cache()
    this.renderer.remove(elem.mesh)
    cache.curves = cache.curves.filter(e => e != elem)
    this.onUnloadElement(elem, comp)
  }

  updateRegions(comp) {
    this.purgeRegions(comp)
    let t = performance.now()
    const regions = comp.real.get_sketch().get_regions()
    console.log('get_regions took ' + (performance.now() - t))
    t = performance.now()
    comp.cache().regions = regions.map(region => {
      // let material = this.renderer.materials.region.clone()
      // material.color = new THREE.Color(Math.random(), Math.random(), Math.random())
      const mesh = this.renderer.convertMesh(
        region.get_mesh(),
        this.renderer.materials.region
      )
      mesh.alcType = 'region'
      mesh.alcObject = region
      region.mesh = mesh
      region.component = comp
      this.renderer.add(mesh)
      return region
    })
    console.log('region tesselation took ' + (performance.now() - t))
  }

  purgeRegions(comp) {
    const cache = comp.cache()
    cache.regions.forEach(region => {
      if(region.noFree) {
        region.unused = true
      } else {
        region.free()
      }
      this.renderer.remove(region.mesh)
    })
    cache.regions = []
  }

  getSurfaceMaterial(comp, face) {
    const material = comp.getMaterial()
    const surfaceMaterial = material ?
      material.displayMaterial :
      this.renderer.materials.surface
    return this.isHighlighted(face) || this.isSelected(face) ?
      this.renderer.materials.highlightSurface :
      this.isActive(comp) ?
        surfaceMaterial : this.renderer.materials.ghostSurface
  }

  getWireMaterial(comp, solid) {
    return this.isHighlighted(solid) || this.isSelected(solid) ?
      this.renderer.materials.selectionLine :
      this.isActive(comp) ?
        this.renderer.materials.wire : this.renderer.materials.ghostWire
  }

  getElemMaterial(elem) {
    const selected = this.isSelected(elem)
    const highlighted = this.isHighlighted(elem)
    return {
      curve: selected ? this.renderer.materials.selectionLine :
        highlighted ? this.renderer.materials.highlightLine :
          this.renderer.materials.line,
      region: highlighted ? this.renderer.materials.highlightRegion :
        this.renderer.materials.region,
      face: highlighted ? this.renderer.materials.highlightSurface :
        this.renderer.materials.surface,
    }[elem.mesh.alcType]
  }

  applyMaterials(obj) {
    if(!obj || obj.deallocated) return
    const comp = this.getComponent(obj)
    const cache = comp.cache()
    for(const solid of comp.solids) {
      const wireMaterial = this.getWireMaterial(comp, solid)
      const solidId = solid.get_id()
      const edges = cache.edges.filter(e => e.get_solid_id() == solidId )
      edges.forEach(edge => edge.mesh.material = wireMaterial )
    }
    cache.faces.forEach(face => face.mesh.material = this.getSurfaceMaterial(comp, face) )
    cache.regions.forEach(region => region.mesh.material = this.getElemMaterial(region) )
    cache.curves.forEach(curve => curve.mesh.material = this.getElemMaterial(curve) )
    comp.children.forEach(child => this.applyMaterials(child) )
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
