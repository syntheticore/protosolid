import * as THREE from 'three'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'

class Materials {
  constructor() {
    // Fat Line Materials
    this.line = new LineMaterial({
      color: 'yellow',
      linewidth: 3,
      vertexColors: true,
      dashed: false,
      // dashScale, dashSize, gapSize
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

    this.referenceLine = this.line.clone()
    this.referenceLine.color.set('#8eca53')
    this.referenceLine.dashed = true
    this.referenceLine.dashScale = 0.5
    this.referenceLine.linewidth = 2

    this.referenceSelectionLine = this.referenceLine.clone()
    this.referenceSelectionLine.color.set('#0070ff')
    this.referenceSelectionLine.toneMapped = false

    this.referenceHighlightLine = this.referenceLine.clone()
    this.referenceHighlightLine.color.set('#2590e1')

    this.projectedLine = this.line.clone()
    this.projectedLine.color.set('#51bb89')

    this.wire = this.line.clone()
    this.wire.color.set('darkgray')
    this.wire.linewidth = 2

    this.ghostWire = this.wire.clone()
    this.ghostWire.color.set('rgb(45, 49, 53)')
    this.ghostWire.linewidth = 1.5

    this.uiWire = this.wire.clone()
    this.uiWire.linewidth = 3

    this.table = {
      curve: {
        unselected: {
          regular: {
            actual: this.line,
            reference: this.referenceLine,
          },
          projected: {
            actual: this.projectedLine,
            reference: this.referenceLine,
          },
        },
        selected: {
          regular: {
            actual: this.selectionLine,
            reference: this.referenceSelectionLine,
          },
          projected: {
            actual: this.selectionLine,
            reference: this.referenceSelectionLine,
          },
        },
        highlighted: {
          regular: {
            actual: this.highlightLine,
            reference: this.referenceHighlightLine,
          },
          projected: {
            actual: this.highlightLine,
            reference: this.referenceHighlightLine,
          },
        },
      },
    }

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
      opacity: 0.04,
      depthTest: false,
    })

    this.highlightPlane = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: '#0070ff',
      transparent: true,
      opacity: 0.6,
      depthTest: false,
    })

    this.ui = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: new THREE.Color('darkgray'),
      // depthTest: false,
    })

    this.highlightUi = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: new THREE.Color('#0070ff'),
    })

    // Surface Materials
    this.surface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide,
      color: '#abc5d9',
      roughness: 0.5,
      metalness: 1.0,
      // wireframe: true,
    })

    this.highlightSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide,
      color: '#0070ff',
      emissive: 'blue',
    })

    this.ghostSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide,
      color: 'white',
      roughness: 1.0,
      transparent: true,
      depthWrite: false,
      opacity: 0.035,
    })

    this.previewAddSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide,
      color: '#0070ff',
      transparent: true,
      opacity: 0.4,
      polygonOffset: true,
      polygonOffsetFactor: -0.5,
    })

    this.previewSubtractSurface = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide,
      color: 'red',
      transparent: true,
      opacity: 0.4,
      // depthTest: false,
      polygonOffset: true,
      polygonOffsetFactor: 0.5,
    })
  }
}

const materials = new Materials()

export default materials
