import * as THREE from 'three'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'

export default class Materials {
  constructor() {
    // Fat Line Materials
    this.line = new LineMaterial({
      color: 'yellow',
      linewidth: 3,
      vertexColors: true,
      dashed: false,
      // precision: "highp",
      polygonOffset: true,
      polygonOffsetFactor: -2,
      // polygonOffsetUnits: 1,
      transparent: true,
      opacity: 1,
    })

    this.selectionLine = this.line.clone()
    this.selectionLine.color.set('#0070ff')
    this.selectionLine.toneMapped = false

    this.highlightLine = this.line.clone()
    this.highlightLine.color.set('#2590e1')

    this.wire = this.line.clone()
    this.wire.color.set('darkgray')
    this.wire.linewidth = 2

    this.ghostWire = this.wire.clone()
    this.ghostWire.color.set('rgb(8, 8, 8)')
    this.ghostWire.linewidth = 1.5

    // Line Materials
    this.lineBasic = new THREE.LineBasicMaterial({
      color: 'gray',
    })

    // Region materials
    this.region = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: new THREE.Color('coral'),
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

    this.plane = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.1,
      depthTest: false,
    })

    this.highlightPlane = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: '#0070ff',
      transparent: true,
      opacity: 0.6,
      depthTest: false,
    })

    // Surface Materials
    this.surface = new THREE.MeshStandardMaterial({
      color: '#53a3e1',
      roughness: 0.25,
      metalness: 0.2,
      polygonOffset: true,
      polygonOffsetFactor: 2,
      // wireframe: true,
    })

    this.highlightSurface = new THREE.MeshStandardMaterial({
      color: '#0070ff',
      emissive: 'blue',
    })

    this.ghostSurface = new THREE.MeshStandardMaterial({
      color: 'white',
      roughness: 1.0,
      transparent: true,
      depthWrite: false,
      opacity: 0.035,
    })

    this.previewAddSurface = new THREE.MeshStandardMaterial({
      color: '#0070ff',
      transparent: true,
      opacity: 0.4,
      polygonOffset: true,
      polygonOffsetFactor: -0.5,
    })

    this.previewSubtractSurface = new THREE.MeshStandardMaterial({
      color: 'red',
      transparent: true,
      opacity: 0.4,
      depthTest: false,
      polygonOffset: true,
      polygonOffsetFactor: 0.5,
    })
  }
}
