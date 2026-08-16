import * as THREE from 'three'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'
import materials from '../materials.js'

const screenPosition = new THREE.Vector3()
const screenQuaternion = new THREE.Quaternion()
const parentQuaternion = new THREE.Quaternion()

function setScreenDiameter(object, camera, renderer, pixelDiameter) {
  const viewportHeight = renderer.domElement.clientHeight
  if(!viewportHeight) return

  let visibleWorldHeight
  if(camera.isOrthographicCamera) {
    visibleWorldHeight = (camera.top - camera.bottom) / camera.zoom
  } else {
    const cameraPosition = object.getWorldPosition(screenPosition).applyMatrix4(camera.matrixWorldInverse)
    const depth = Math.abs(cameraPosition.z)
    const fov = THREE.MathUtils.degToRad(camera.getEffectiveFOV())
    visibleWorldHeight = 2.0 * depth * Math.tan(fov / 2.0)
  }

  const size = pixelDiameter * visibleWorldHeight / viewportHeight
  if(Number.isFinite(size)) {
    camera.getWorldQuaternion(screenQuaternion)
    if(object.parent) {
      object.parent.getWorldQuaternion(parentQuaternion).invert()
      object.quaternion.copy(parentQuaternion.multiply(screenQuaternion))
    } else {
      object.quaternion.copy(screenQuaternion)
    }
    object.scale.set(size, size, 1)
    // onBeforeRender runs after the scene's normal matrix-world update. Make
    // this frame use the new billboard size instead of waiting for the next
    // camera interaction to refresh it.
    object.updateMatrix()
    object.updateMatrixWorld(true)
  }
}


export class ScreenPointObject extends THREE.Mesh {
  constructor(pixelDiameter, material=materials.uiPoint) {
    super(new THREE.CircleGeometry(0.5, 32), material)
    this.pixelDiameter = pixelDiameter
    this.renderOrder = 100
  }

  onBeforeRender(renderer, scene, camera) {
    setScreenDiameter(this, camera, renderer, this.pixelDiameter)
  }
}


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

export class PointHelperObject extends ScreenPointObject {
  constructor(alcObject) {
    super(16)

    this.alcTypes = ['point']
    this.alcObject = alcObject

    this.applyMatrix4(alcObject.transform)
  }

  setMaterial(highlighted, selected) {
    this.material = (highlighted || selected) ? materials.highlightUiPoint : materials.uiPoint
  }

}
