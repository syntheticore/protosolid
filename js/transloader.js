import * as THREE from 'three'

import Component from './component.js'

export default class Transloader {
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
    const cache = comp.cache()
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
          faceMesh.alcObject = face
          face.mesh = faceMesh
          // faceMesh.alcComponent = comp
          faceMesh.alcProjectable = isActive
          faceMesh.castShadow = isActive
          faceMesh.receiveShadow = isActive
          this.renderer.add(faceMesh)
          cache.faces.push(faceMesh)
          // const normal = this.convertLine(face.get_display_normal(), this.renderer.materials.selectionLine)
          // this.renderer.add(normal)
        })
      }
      // Load Edges
      if(mode == 'wireframe' || (isActive && mode == 'wireShade')) {
        const edges = solid.get_edges()
        const wireMaterial = this.getWireMaterial(comp)
        cache.edges = (cache.edges || []).concat(edges.map(edge => {
          const line = this.renderer.convertLine(edge.tesselate(), wireMaterial)
          line.alcType = 'edge'
          line.alcObject = edge
          edge.mesh = line
          this.renderer.add(line)
          return line
        }))
      }
    })
    // Load Sketch Elements
    if(comp === this.document.activeComponent) {
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
    cache.edges.forEach(edge => this.renderer.remove(edge))
    cache.faces.forEach(faceMesh => this.renderer.remove(faceMesh))
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
    const regions = comp.real.get_sketch().get_regions(false)
    // console.log('# regions: ', regions.length)
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
      this.renderer.add(mesh)
      return mesh
    })
  }

  purgeRegions(comp) {
    const cache = comp.cache()
    cache.regions.forEach(mesh => {
      if(mesh.alcObject.noFree) {
        mesh.alcObject.unused = true
      } else {
        mesh.alcObject.free()
      }
      this.renderer.remove(mesh)
    })
    cache.regions = []
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
    const cache = comp.cache()
    const surfaceMaterial = this.getSurfaceMaterial(comp, highlight)
    const wireMaterial = this.getWireMaterial(comp, highlight)
    const faces = solidId ?
      cache.faces.filter(f => f.alcObject.get_solid_id() == solidId) :
      cache.faces
    const edges = solidId ?
      cache.edges.filter(e => e.alcObject.get_solid_id() == solidId) :
      cache.edges
    faces.forEach(face => face.material = surfaceMaterial )
    edges.forEach(edge => edge.material = wireMaterial )
    if(!solidId) comp.children.forEach(child => this.applyMaterials(child, highlight))
  }

  highlightComponent(comp, solidId) {
    this.applyMaterials(comp, true, solidId)
  }

  unhighlightComponent(comp, solidId) {
    this.applyMaterials(comp, false, solidId)
  }

  select(selection) {
    if(!selection || selection.deallocated) return
    if(selection.constructor === Component) {
      this.highlightComponent(selection)
    } else if(selection.constructor === window.alcWasm.JsSolid) {
      this.highlightComponent(selection.component, selection.get_id())
    } else {
      selection.mesh.material = this.renderer.materials.selectionLine
    }
  }

  unselect(selection) {
    if(!selection || selection.deallocated) return
    if(selection.constructor === Component) {
      this.unhighlightComponent(selection)
    } else if(selection.constructor === window.alcWasm.JsSolid) {
      this.unhighlightComponent(selection.component, selection.get_id())
    } else {
      selection.mesh.material = this.renderer.materials.line
    }
  }

  highlight(obj) {
    if(!obj || obj.deallocated) return
    if(obj.constructor === Component) {
      this.highlightComponent(obj)
    } else if(obj.constructor === window.alcWasm.JsSolid) {
      this.highlightComponent(obj.component, obj.get_id())
    } else {
      obj.mesh.material = {
        curve: this.renderer.materials.highlightLine,
        region: this.renderer.materials.highlightRegion,
        face: this.renderer.materials.highlightSurface,
      }[obj.mesh.alcType]
    }
  }

  unhighlight(obj) {
    if(!obj || obj.deallocated) return
    if(obj.constructor === Component) {
      this.unhighlightComponent(obj)
    } else if(obj.constructor === window.alcWasm.JsSolid) {
      this.unhighlightComponent(obj.component, obj.get_id())
    } else {
      if(!obj.mesh) return
      obj.mesh.material = {
        curve: this.renderer.materials.line,
        region: this.renderer.materials.region,
        face: this.renderer.materials.surface,
      }[obj.mesh.alcType]
    }
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
