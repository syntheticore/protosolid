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
    this.highlightLine.depthTest = false

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

    this.splineControlLine = this.line.clone()
    this.splineControlLine.color.set('#8a939b')
    this.splineControlLine.dashed = true
    this.splineControlLine.dashScale = 0.5
    this.splineControlLine.linewidth = 1.5
    this.splineControlLine.depthTest = false

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

    this.invisibleLine = new LineMaterial({
      linewidth: 3,
      transparent: true,
      opacity: 0,
    })

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
      depthWrite: false,
    })

    this.highlightRegion = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      color: new THREE.Color('#0090ff'),
      transparent: true,
      opacity: 0.4,
      polygonOffset: true,
      polygonOffsetFactor: -1,
      depthTest: false,
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

    this.uiPoint = new THREE.MeshBasicMaterial({
      color: new THREE.Color('darkgray'),
      side: THREE.DoubleSide,
      transparent: true,
      depthTest: false,
      depthWrite: false,
    })

    this.highlightUiPoint = this.uiPoint.clone()
    this.highlightUiPoint.color.set('#0070ff')

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
      depthWrite: false,
    })

    this.invisibleSurface = new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0,
      depthWrite: false,
    })

    this.diagnostic = {
      zebra: this.makeDiagnosticSurface('zebra', [0, 0, 1]),
      draft: {
        top: this.makeDiagnosticSurface('draft', [0, 1, 0]),
        front: this.makeDiagnosticSurface('draft', [0, 0, 1]),
        side: this.makeDiagnosticSurface('draft', [1, 0, 0]),
      },
    }
  }

  makeDiagnosticSurface(mode, direction) {
    const material = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide,
      color: '#abc5d9',
      roughness: 0.65,
      metalness: 0.0,
    })
    let frequency = 8
    const diagnosticDirection = new THREE.Vector3(...direction)
    material.setFrequency = value => {
      frequency = value
      if(material.userData.diagnosticShader) {
        material.userData.diagnosticShader.uniforms.diagnosticFrequency.value = value
      }
    }
    material.customProgramCacheKey = () => mode + (direction || []).join(',')
    material.onBeforeCompile = shader => {
      shader.uniforms.diagnosticFrequency = { value: frequency }
      shader.uniforms.diagnosticDirection = { value: diagnosticDirection }
      material.userData.diagnosticShader = shader
      shader.vertexShader = shader.vertexShader
        .replace(
          '#include <common>',
          '#include <common>\nuniform vec3 diagnosticDirection;\nvarying vec3 diagnosticViewDirection;'
        )
        .replace(
          '#include <defaultnormal_vertex>',
          '#include <defaultnormal_vertex>\ndiagnosticViewDirection = normalize(mat3(viewMatrix) * diagnosticDirection);'
        )

      const shading = mode == 'zebra' ?
        `
          vec3 diagnosticView = normalize(vViewPosition);
          vec3 diagnosticReflection = reflect(-diagnosticView, normal);
          float stripe = step(0.0, sin(dot(diagnosticReflection, normalize(vec3(1.0, 0.65, 0.35))) * diagnosticFrequency));
          diffuseColor.rgb = mix(vec3(0.03), vec3(0.95), stripe);
        `
        :
        `
          float draftAngle = acos(clamp(dot(normal, normalize(diagnosticViewDirection)), -1.0, 1.0));
          float warning = smoothstep(0.0, radians(45.0), draftAngle);
          float failure = smoothstep(radians(45.0), radians(90.0), draftAngle);
          diffuseColor.rgb = mix(vec3(0.1, 0.75, 0.25), vec3(1.0, 0.8, 0.05), warning);
          diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.9, 0.1, 0.05), failure);
        `
      shader.fragmentShader = shader.fragmentShader
        .replace(
          '#include <common>',
          '#include <common>\nuniform float diagnosticFrequency;\nvarying vec3 diagnosticViewDirection;'
        )
        .replace('#include <opaque_fragment>', `${shading}\n\toutgoingLight = diffuseColor.rgb;\n\t#include <opaque_fragment>`)
    }
    return material
  }
}

const materials = new Materials()

export default materials
