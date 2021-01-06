import * as THREE from 'three'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'

export class Materials {
  constructor() {
    // Fat Line Materials
    this.line = new LineMaterial({
      color: 'yellow',
      linewidth: 3,
      vertexColors: true,
      dashed: false,
      polygonOffset: true,
      polygonOffsetFactor: -2,
    })

    this.selectionLine = this.line.clone()
    this.selectionLine.color.set('#0070ff')

    this.highlightLine = this.line.clone()
    this.highlightLine.color.set('#2590e1')

    this.wire = this.line.clone()
    this.wire.color.set('darkgray')
    this.wire.linewidth = 2

    // Line Materials
    this.lineBasic = new THREE.LineBasicMaterial({
      color: 'gray',
    })

    // Region materials
    this.region = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: new THREE.Color('coral'),
      // depthTest: false,
      transparent: true,
      opacity: 0.1,
      polygonOffset: true,
      polygonOffsetFactor: -1,
    })

    this.highlightRegion = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: new THREE.Color('#0090ff'),
      transparent: true,
      opacity: 0.4,
      polygonOffset: true,
      polygonOffsetFactor: -1,
    })

    // Surface Materials
    this.surface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide, //XXX remove
      color: '#53a3e1',
      roughness: 0.25,
      metalness: 0.2,
    })

    this.highlightSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide, //XXX remove
      color: '#0070ff',
    })

    this.previewAddSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide, //XXX remove
      color: '#0090ff',
      transparent: true,
      opacity: 0.4,
      polygonOffset: true,
      polygonOffsetFactor: -0.5,
    })

    this.previewSubtractSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide, //XXX remove
      color: 'red',
      transparent: true,
      opacity: 0.4,
      polygonOffset: true,
      polygonOffsetFactor: -0.5,
    })
  }
}
