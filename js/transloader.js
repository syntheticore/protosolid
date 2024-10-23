// import * as THREE from 'three'
import { VertexNormalsHelper } from 'three/examples/jsm/helpers/VertexNormalsHelper.js'

import Component from './core/component.js'
import PlaneHelperObject from './three/plane-helper-object.js'
import DimensionControls from './three/dimension-controls.js'
import { Edge, Face, Solid, Profile, Sketch, SketchElement, ConstructionHelper, Dimension } from './core/kernel.js'

let vnhs = [];

export default class Transloader {
  constructor(renderer, onLoadElement, onUnloadElement) {
    this.renderer = renderer
    this.onLoadElement = onLoadElement
    this.onUnloadElement = onUnloadElement
    this.selection = []
  }

  setDocument(doc) {
    this.document = doc
  }

  setSelection(selection) {
    const old = this.selection
    this.selection = [...selection.set]
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
    return this.hasAncestor(obj, this.document.activeComponent)
  }

  isSelected(obj) {
    return this.selection.some(item => this.hasAncestor(obj, item) )
  }

  isHighlighted(obj) {
    if(!this.highlight) return false
    return this.hasAncestor(obj, this.highlight)
  }

  hasAncestor(obj, ancestor) {
    if(obj === ancestor || (ancestor && obj.id && obj.id == ancestor.id)) return true
    if(obj.constructor === Edge || obj.constructor === Face) return this.hasAncestor(obj.solid, ancestor)
    if(obj instanceof SketchElement) return this.hasAncestor(obj.sketch, ancestor)
    return this.getComponent(obj).hasAncestor(ancestor)
  }

  getComponent(obj) {
    if(obj instanceof Component) {
      return obj
    } else if(obj instanceof Solid || obj instanceof Profile || obj instanceof SketchElement || obj instanceof Sketch || obj instanceof ConstructionHelper) {
      return obj.component
    } else if(obj instanceof Edge || obj instanceof Face) {
      return obj.solid.component
    }
  }

  getComponents(objs) {
    return objs.map(obj => this.getComponent(obj) )
    .filter((value, index, self) => value && self.indexOf(value) === index )
  }

  loadTree(comp, recursive) {
    if(comp.creator.hidden) return
    vnhs.forEach(vnh => this.renderer.remove(vnh) )

    // Load Bodies
    const isActive = this.isActive(comp)
    const cache = comp.creator.cache()
    comp.compound.solids().forEach(solid => {
      solid.component = comp

      // Load Faces
      const mode = this.renderer.displayMode
      if(mode == 'shaded' || mode == 'wireShade') {
        const faces = solid.faces()
        faces.forEach(face => {
          // face.solid = solid
          const faceMesh = this.renderer.convertMesh(
            face.tesselate(),
            this.getSurfaceMaterial(comp, face),
          )
          face.mesh = () => faceMesh
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
        cache.edges = (cache.edges || []).concat(edges.map(edge => {
          const line = this.renderer.convertLine(edge.tesselate(), this.getWireMaterial(comp, edge))
          line.alcType = 'edge'
          line.alcObject = edge
          edge.mesh = () => line
          // edge.solid = solid
          this.renderer.add(line, true)
          return edge
        }))
      }
    })

    // if(comp === this.document.activeComponent) {

      // Load Sketches
      comp.sketches.forEach(sketch => {
        sketch.component = comp
        if(comp.creator.itemsHidden[sketch.id]) return

        // Load Sketch Elements
        sketch.elements.forEach(elem => {
          this.loadElement(elem, comp)
        })

        // Load Projections
        sketch.projections.forEach(projection => {
          const elem = projection.geometry()
          if(elem) this.loadElement(elem, comp)
        })

        // Load Dimensions
        if(sketch == this.document.activeSketch) this.updateDimensions(comp, sketch)
      })

      // Update regions
      if(!cache.regions.length) this.updateRegions(comp)

    // }

    // Load Construction Helpers
    comp.helpers.forEach(plane => {
      const mesh = new PlaneHelperObject(plane)
      plane.mesh = () => mesh
      plane.component = comp
      this.renderer.add(mesh, true)
      cache.helpers.push(plane)
    })

    // Recurse
    if(recursive) comp.children.forEach(child => this.loadTree(child, true))
  }

  unloadTree(comp, recursive) {
    const cache = comp.creator.cache()

    cache.curves.forEach(elem => this.unloadElement(elem, comp))

    cache.edges.forEach(edge => {
      this.renderer.remove(edge.mesh())
    })
    cache.edges = []

    cache.faces.forEach(face => {
      this.renderer.remove(face.mesh())
    })
    cache.faces = []

    cache.helpers.forEach(helper => {
      this.renderer.remove(helper.mesh())
    })
    cache.helpers = []

    this.purgeDimensions(comp)

    this.purgeRegions(comp)

    if(recursive) comp.children.forEach(child =>
      this.unloadTree(child, true)
    )
  }

  loadElement(elem, comp) {
    this.unloadElement(elem, comp)
    const vertices = elem.tesselate()
    if(!vertices) return
    const line = this.renderer.convertLine(vertices, this.renderer.materials.line)
    line.applyMatrix4(elem.sketch.workplane)
    line.alcType = 'curve'
    line.alcObject = elem
    elem.mesh = () => line
    elem.component = comp
    line.material = this.getElemMaterial(elem)
    this.renderer.add(line, true)
    comp.creator.cache().curves.push(elem)
    this.onLoadElement(elem)
  }

  unloadElement(elem, comp) {
    if(elem.mesh) {
      const cache = comp.creator.cache()
      this.renderer.remove(elem.mesh())
      cache.curves = cache.curves.filter(e => e != elem )
    }
    this.onUnloadElement(elem, comp)
  }

  updateRegions(comp) {
    this.purgeRegions(comp)
    let t = performance.now()
    const regions = comp.sketches.filter(sketch => !comp.creator.itemsHidden[sketch.id] ).flatMap(sketch => sketch.profiles(comp) )
    // console.log('get_profiles took ' + (performance.now() - t))
    t = performance.now()
    comp.creator.cache().regions = regions
    regions.forEach(region => {
      // let material = this.renderer.materials.region.clone()
      // material.color = new THREE.Color(Math.random(), Math.random(), Math.random())
      const mesh = this.renderer.convertMesh(
        region.tesselate(),
        this.renderer.materials.region
      )
      mesh.applyMatrix4(region.sketch.workplane)
      mesh.alcType = 'region'
      mesh.alcObject = region
      region.mesh = () => mesh
      region.component = comp
      this.renderer.add(mesh, true)
    })
    // console.log('region tesselation took ' + (performance.now() - t))
  }

  purgeRegions(comp) {
    const cache = comp.creator.cache()
    cache.regions.forEach(region => {
      // region.free()
      this.renderer.remove(region.mesh())
    })
    cache.regions = []
  }

  updateDimensions(comp, sketch) {
    const cache = comp.creator.cache()
    this.purgeDimensions(comp)
    sketch.constraints.filter(c => c instanceof Dimension).forEach(constraint => {
      const dimension = new DimensionControls(constraint, this.renderer)
      constraint.controls = () => dimension
      this.renderer.add(dimension)
      cache.dimensions.push(constraint)
    })
  }

  purgeDimensions(comp, sketch) {
    const cache = comp.creator.cache();
    ((sketch && sketch.constraints.filter(c => c instanceof Dimension)) || [...cache.dimensions]).forEach(dimension => {
      this.renderer.remove(dimension.controls())
      cache.dimensions.splice(cache.dimensions.indexOf(dimension), 1)
    })
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

  getWireMaterial(comp, edge) {
    return this.isHighlighted(edge) || this.isSelected(edge) ?
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
          elem.projection ? this.renderer.materials.projectedLine :
            elem.isReference ? this.renderer.materials.referenceLine :
              this.renderer.materials.line,
      region: highlighted ? this.renderer.materials.highlightRegion :
        this.renderer.materials.region,
      plane: highlighted ? this.renderer.materials.highlightPlane :
        this.renderer.materials.plane,
      face: highlighted ? this.renderer.materials.highlightSurface :
        this.renderer.materials.surface,
    }[elem.mesh().alcType]
  }

  applyMaterials(obj) {
    if(!obj) return
    const comp = this.getComponent(obj)
    if(!comp) return
    const cache = comp.creator.cache()
    // for(const solid of comp.compound.solids()) {
    //   const edges = cache.edges.filter(e => e.solid.id === solid.id )
    //   edges.forEach(edge => edge.mesh().material = this.getWireMaterial(comp, edge) )
    // }
    cache.edges.forEach(edge => edge.mesh().material = this.getWireMaterial(comp, edge) )
    cache.faces.forEach(face => face.mesh().material = this.getSurfaceMaterial(comp, face) )
    cache.regions.forEach(region => region.mesh().material = this.getElemMaterial(region) )
    cache.curves.forEach(curve => curve.mesh().material = this.getElemMaterial(curve) )
    comp.helpers.forEach(helper => helper.mesh().material = this.getElemMaterial(helper) )
    comp.children.forEach(child => this.applyMaterials(child) )
  }

  previewFeature(bufferGeometry) {
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
