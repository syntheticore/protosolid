import * as THREE from 'three'
import {
  matrix2three,
} from './utils.js'


export default class PlaneHelper extends THREE.Mesh {
  constructor(alcObject) {
    var groundGeo = new THREE.PlaneBufferGeometry(20, 20)

    super(groundGeo, new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.1,
      depthTest: false,
    }))

    this.alcType = 'plane'
    this.alcObject = alcObject

    this.applyMatrix4(matrix2three(alcObject.transform()))
  }
}
