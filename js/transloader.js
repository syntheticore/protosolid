import * as THREE from 'three'
import { VertexNormalsHelper } from 'three/examples/jsm/helpers/VertexNormalsHelper.js'

import Component from './component.js'
import PlaneHelper from './plane-helper.js'

let vnhs = [];

export default class Transloader {
  constructor(renderer, onLoadElement, onUnloadElement) {
    this.renderer = renderer
    this.onLoadElement = onLoadElement
    this.onUnloadElement = onUnloadElement
    this.selection = []
  }

  setActiveComponent(comp) {
    this.activeComponent = comp
  }

  setActiveSketch(sketch) {
    this.activeSketch = sketch
  }

  setSelection(selection) {
    const old = this.selection
    this.selection = [...selection.set]
    const uniqueComps = this.selection.map(obj => this.getComponent(obj) )
      .filter((value, index, self) => self.indexOf(value) === index )
    this.getComponents(old).forEach(comp => this.applyMaterials(comp) )
    this.getComponents(this.selection).forEach(comp => this.applyMaterials(comp) )
  }

  setHighlight(obj) {
    const old = this.highlight
    this.highlight = obj
    this.applyMaterials(old)
    this.applyMaterials(obj)
  }

  isActive(obj) {
    return this.hasAncestor(obj, this.activeComponent)
  }

  isSelected(obj) {
    return this.selection.some(item => this.hasAncestor(obj, item) )
  }

  isHighlighted(obj) {
    return this.hasAncestor(obj, this.highlight)
  }

  hasAncestor(obj, ancestor) {
    if(obj === ancestor) return true
    if(obj.constructor === alcWasm.JsFace) return this.hasAncestor(obj.solid, ancestor)
    return this.getComponent(obj).hasAncestor(ancestor)
  }

  getComponent(obj) {
    switch(obj.constructor) {
      case Component:
        return obj

      case alcWasm.JsSolid:
      case alcWasm.JsRegion:
      case alcWasm.JsCurve:
      case alcWasm.JsConstructionHelper:
        return obj.component

      case alcWasm.JsFace:
        return obj.solid.component
    }
  }

  getComponents(objs) {
    return objs.map(obj => this.getComponent(obj) )
    .filter((value, index, self) => value && self.indexOf(value) === index )
  }

  loadTree(comp, recursive) {
    if(comp.UIData.hidden) return
    vnhs.forEach(vnh => this.renderer.remove(vnh) )
    // Load Bodies
    const isActive = this.isActive(comp)
    comp.updateSolids()
    const cache = comp.cache()
    comp.solids.forEach(solid => {
      const mode = this.renderer.displayMode
      // Load Faces
      if(mode == 'shaded' || mode == 'wireShade') {
        const faces = solid.faces()
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
          this.renderer.add(faceMesh, true)
          cache.faces.push(face)
          const vnh = new VertexNormalsHelper( faceMesh, 5, 0xff0000 )
          // this.renderer.add( vnh )
          // vnhs.push(vnh)
          // const normal = this.convertLine(face.display_normal(), this.renderer.materials.selectionLine)
          // this.renderer.add(normal)
        })
      }
      // Load Edges
      if(mode == 'wireframe' || (isActive && mode == 'wireShade')) {
        const edges = solid.edges()
        const wireMaterial = this.getWireMaterial(comp, solid)
        cache.edges = (cache.edges || []).concat(edges.map(edge => {
          const line = this.renderer.convertLine(edge.tesselate(), wireMaterial)
          line.alcType = 'edge'
          line.alcObject = edge
          edge.mesh = line
          edge.solid = solid
          this.renderer.add(line, true)
          return edge
        }))
      }
    })
    // Load Sketch Elements
    if(comp === this.activeComponent) {
      const elements = comp.sketches.filter(sketch => !comp.UIData.itemsHidden[sketch.id()] ).flatMap(sketch => sketch.sketch_elements() )
      elements.forEach(element => this.loadElement(element, comp))
      if(!cache.regions.length) this.updateRegions(comp)
    }
    // Load Construction Helpers
    comp.helpers.forEach(plane => {
      const mesh = new PlaneHelper(plane)
      plane.mesh = mesh
      this.renderer.add(mesh, true)
    })
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
    cache.edges = []

    cache.faces.forEach(face => {
      this.renderer.remove(face.mesh)
      face.free()
    })
    cache.faces = []

    comp.helpers.forEach(helper => {
      this.renderer.remove(helper.mesh)
    })

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
    this.renderer.add(line, true)
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
    const regions = comp.sketches.filter(sketch => !comp.UIData.itemsHidden[sketch.id()] ).flatMap(sketch => sketch.profiles())
    // console.log('get_profiles took ' + (performance.now() - t))
    t = performance.now()
    comp.cache().regions = regions
    regions.forEach(region => {
      // let material = this.renderer.materials.region.clone()
      // material.color = new THREE.Color(Math.random(), Math.random(), Math.random())
      const mesh = this.renderer.convertMesh(
        region.mesh(),
        this.renderer.materials.region
      )
      mesh.alcType = 'region'
      mesh.alcObject = region
      region.mesh = mesh
      region.component = comp
      this.renderer.add(mesh, true)
    })
    // console.log('region tesselation took ' + (performance.now() - t))
  }

  purgeRegions(comp) {
    const cache = comp.cache()
    cache.regions.forEach(region => {
      region.free()
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
      plane: highlighted ? this.renderer.materials.highlightPlane :
        this.renderer.materials.plane,
      face: highlighted ? this.renderer.materials.highlightSurface :
        this.renderer.materials.surface,
    }[elem.mesh.alcType]
  }

  applyMaterials(obj) {
    if(!obj || obj.deallocated) return
    const comp = this.getComponent(obj)
    if(!comp) return
    const cache = comp.cache()
    for(const solid of comp.solids) {
      const wireMaterial = this.getWireMaterial(comp, solid)
      const edges = cache.edges.filter(e => e.solid === solid )
      edges.forEach(edge => edge.mesh.material = wireMaterial )
    }
    cache.faces.forEach(face => face.mesh.material = this.getSurfaceMaterial(comp, face) )
    cache.regions.forEach(region => region.mesh.material = this.getElemMaterial(region) )
    cache.curves.forEach(curve => curve.mesh.material = this.getElemMaterial(curve) )
    comp.helpers.forEach(helper => helper.mesh.material = this.getElemMaterial(helper) )
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
