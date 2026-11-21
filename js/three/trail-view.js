import * as THREE from 'three'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'

import materials from '../materials.js'
import { ScreenPointObject } from './helper-objects.js'

// Readable on both viewport backgrounds, apart from the selection blue
const colors = ['#ff7a1a', '#16b364', '#e5399a', '#8a5cf6', '#e8b100']

// Paths of the traced points of an animation and where the points are now,
// in world coordinates
export default class TrailView extends THREE.Group {
  constructor() {
    super()
    this.pointMaterials = colors.map(color => new THREE.MeshBasicMaterial({
      color,
      side: THREE.DoubleSide,
      transparent: true,
      depthTest: false,
      depthWrite: false,
    }))
  }

  // [{ trail: [Vector3], position: Vector3 }]
  update(tracers) {
    this.clear()
    tracers.forEach(({ trail, position }, index) => {
      if(trail.length > 1) {
        const geometry = new LineGeometry()
        const positions = trail.flatMap(point => point.toArray())
        const color = new THREE.Color(colors[index % colors.length]).toArray()
        geometry.setPositions(positions)
        geometry.setColors(trail.flatMap(() => color))
        const line = new Line2(geometry, materials.trail)
        line.renderOrder = 90
        this.add(line)
      }
      const marker = new ScreenPointObject(9, this.pointMaterials[index % colors.length])
      marker.position.copy(position)
      this.add(marker)
    })
  }

  clear() {
    this.children.forEach(child => child.geometry.dispose())
    return super.clear()
  }

  dispose() {
    this.clear()
    this.pointMaterials.forEach(material => material.dispose())
  }
}
