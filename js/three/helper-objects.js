import * as THREE from 'three'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'
import materials from '../materials.js'


export class PlaneHelperObject extends THREE.Mesh {
  constructor(alcObject) {
    var geo = new THREE.PlaneGeometry(20, 20)

    super(geo, materials.plane)

    this.alcTypes = ['plane']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(highlighted, selected) {
    this.material = (highlighted || selected) ? materials.highlightPlane : materials.plane
  }
}

export class AxisHelperObject extends Line2 {
  constructor(alcObject) {
    const geometry = new LineGeometry()
    const positions = [[0, 0, 0], [0, 0, 10]].flat()
    geometry.setPositions(positions)
    geometry.setColors(Array(positions.length).fill(1))
    super(geometry, materials.uiWire)
    this.computeLineDistances()

    this.alcTypes = ['axis']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(highlighted, selected) {
    this.material = highlighted ? materials.highlightLine : materials.uiWire
  }
}

export class PointHelperObject extends THREE.Mesh {
  constructor(alcObject) {
    super(new THREE.SphereGeometry(1.05, 32, 16), materials.ui)

    this.alcTypes = ['point']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(highlighted, selected) {
    this.material = highlighted ? materials.highlightUi : materials.ui
  }

  onBeforeRender(renderer, scene, camera) {
    const pos = this.getWorldPosition(new THREE.Vector3())
    const scale = pos.distanceTo(camera.position) / 90.0
    this.scale.set(scale, scale, scale)
  }
}
