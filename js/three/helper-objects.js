import * as THREE from 'three'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'


export class PlaneHelperObject extends THREE.Mesh {
  constructor(alcObject, renderer) {
    var geo = new THREE.PlaneGeometry(20, 20)

    super(geo, renderer.materials.plane)

    this.alcTypes = ['plane']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(renderer, highlighted, selected) {
    this.material = highlighted ? renderer.materials.highlightPlane : renderer.materials.plane
  }
}

export class AxisHelperObject extends Line2 {
  constructor(alcObject, renderer) {
    const geometry = new LineGeometry()
    const positions = [[0, 0, 0], [0, 0, 10]].flat()
    geometry.setPositions(positions)
    geometry.setColors(Array(positions.length).fill(1))
    super(geometry, renderer.materials.uiWire)
    this.computeLineDistances()

    this.alcTypes = ['axis']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(renderer, highlighted, selected) {
    this.material = highlighted ? renderer.materials.highlightLine : renderer.materials.uiWire
  }
}

export class PointHelperObject extends THREE.Mesh {
  constructor(alcObject, renderer) {
    super(new THREE.SphereGeometry(1.25, 32, 16), renderer.materials.ui)

    this.alcTypes = ['point']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(renderer, highlighted, selected) {
    this.material = highlighted ? renderer.materials.highlightUi : renderer.materials.ui
  }

  onBeforeRender(renderer, scene, camera) {
    const pos = this.getWorldPosition(new THREE.Vector3())
    const scale = pos.distanceTo(camera.position) / 90.0
    this.scale.set(scale, scale, scale)
  }
}
