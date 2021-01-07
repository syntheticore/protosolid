import * as THREE from 'three'

export class Transloader {
  constructor(renderer, dataPool, onLoadElement, onUnloadElement) {
    this.renderer = renderer
    this.dataPool = dataPool
    this.onLoadElement = onLoadElement
    this.onUnloadElement = onUnloadElement
  }

  loadTree(node, recursive) {
    const compData = this.dataPool[node.id()]
    this.unloadTree(node, recursive)
    compData.regions.forEach(mesh => this.renderer.remove(mesh))
    if(compData.hidden) return
    let solids = node.get_solids()
    solids.forEach(solid => {
      const faces = solid.get_faces()
      faces.forEach(face => {
        const faceMesh = this.renderer.convertMesh(
          face.tesselate(),
          this.renderer.materials.surface
        )
        faceMesh.alcType = 'face'
        faceMesh.alcFace = face
        faceMesh.alcComponent = node
        faceMesh.alcProjectable = true
        faceMesh.castShadow = true
        faceMesh.receiveShadow = true
        this.renderer.add(faceMesh)
        compData.faces.push(faceMesh)
        // const normal = this.convertLine(face.get_normal(), this.renderer.materials.selectionLine)
        // this.renderer.add(normal)
      })
      const wireframe = solid.get_edges()
      compData.wireframe = (compData.wireframe || []).concat(wireframe.map(edge => {
        // edge = edge.map(vertex => vertex.map(dim => dim + Math.random() / 5))
        const line = this.renderer.convertLine(edge, this.renderer.materials.wire)
        this.renderer.add(line)
        return line
      }))
    })
    this.updateRegions(node)
    // Load sketch elements
    const elements = node.get_sketch().get_sketch_elements()
    elements.forEach(element => this.loadElement(element, node))
    if(recursive) node.get_children().forEach(child => this.loadTree(child, true))
  }

  unloadTree(node, recursive) {
    const nodeData = this.dataPool[node.id()]
    nodeData.curves.forEach(elem => this.unloadElement(elem, node))
    nodeData.wireframe.forEach(edge => this.renderer.remove(edge))
    nodeData.faces.forEach(faceMesh => this.renderer.remove(faceMesh))
    this.purgeRegions(nodeData)
    this.renderer.remove(this.previewMesh)
    if(recursive) node.get_children().forEach(child =>
      this.unloadTree(child, true)
    )
  }

  loadElement(elem, node) {
    this.unloadElement(elem, node)
    const line = this.renderer.convertLine(elem.tesselate(), this.renderer.materials.line)
    line.alcType = 'curve'
    line.alcElement = elem
    this.renderer.add(line)
    this.dataPool[elem.id()] = line
    this.dataPool[node.id()].curves.push(elem)
    this.onLoadElement(elem, node)
  }

  unloadElement(elem, node) {
    this.renderer.remove(this.dataPool[elem.id()])
    const nodeId = node.id()
    const curves = this.dataPool[nodeId].curves
    this.dataPool[nodeId].curves = curves.filter(e => e != elem)
    this.onUnloadElement(elem, node)
  }

  updateRegions(comp) {
    const compData = this.dataPool[comp.id()]
    this.purgeRegions(compData)
    const regions = comp.get_sketch().get_regions(false)
    console.log('# regions: ', regions.length)
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
}
