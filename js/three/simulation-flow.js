import * as THREE from 'three'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js'
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js'


export default class SimulationFlow extends THREE.Group {
  constructor(simulation) {
    super()
    const fluid = simulation.mode == 'fluid'
    const samples = fluid ? (simulation.showFlowVectors ? simulation.flowSamples() : []) : simulation.fluxSamples()
    const maximum = simulation.mode == 'fluid'
      ? simulation.result.maxSpeed
      : Math.max(...samples.map(sample => sample.flux.length()))
    const slowColor = new THREE.Color(0x003cff)
    const fastColor = new THREE.Color(0xff0000)
    samples.forEach(sample => {
      const strength = sample.flux.length() / maximum
      const length = simulation.result.step * (1 + strength * 1.5)
      const arrow = new THREE.ArrowHelper(
        sample.flux.clone().normalize(),
        sample.position,
        length,
        fluid ? slowColor.clone().lerp(fastColor, strength) : 0xffffff,
        length * 0.22,
        length * 0.1,
      )
      arrow.line.material.depthTest = false
      arrow.cone.material.depthTest = false
      arrow.line.renderOrder = 10
      arrow.cone.renderOrder = 10
      this.add(arrow)
    })
    if(fluid && simulation.showFlowLines) {
      const positions = []
      const colors = []
      const color = new THREE.Color()
      simulation.flowLines().forEach(line => {
        for(let i = 1; i < line.length; i++) {
          for(const sample of [line[i - 1], line[i]]) {
            positions.push(sample.position.x, sample.position.y, sample.position.z)
            color.copy(slowColor).lerp(fastColor, sample.speed / maximum)
            colors.push(color.r, color.g, color.b)
          }
        }
      })
      const geometry = new LineSegmentsGeometry()
      geometry.setPositions(positions)
      geometry.setColors(colors)
      const material = new LineMaterial({ vertexColors: true, depthTest: false, linewidth: 2 })
      const lines = new LineSegments2(geometry, material)
      lines.renderOrder = 10
      this.add(lines)
    }
  }

  dispose() {
    super.dispose()
    this.traverse(object => {
      object.geometry?.dispose()
      object.material?.dispose()
    })
  }
}
