import * as THREE from 'three'

export function saveFile(data, filename, filetype) {
  var blob = new Blob([data], {filetype})
  saveBlob(blob, filename, filetype)
}

export function saveBlob(blob, filename, filetype) {
  filename = filename.replace(new RegExp(' ', 'g'), '_') + '.' + filetype
  const a = document.createElement("a")
  const url = URL.createObjectURL(blob)
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  setTimeout(function() {
    document.body.removeChild(a)
    window.URL.revokeObjectURL(url)
  }, 0)
}

export function rotationFromNormal(normal) {
  let up = THREE.Object3D.DefaultUp
  let axis
  if(Math.abs(normal.z - 1) < 0.000000001 || Math.abs(normal.z + 1) < 0.000000001) {
    axis = new THREE.Vector3(1, 0, 0)
  } else {
    axis = new THREE.Vector3().crossVectors(up, normal)
  }
  let radians = Math.acos(Math.min(1, Math.max(-1, normal.dot(up))))
  return new THREE.Quaternion().setFromAxisAngle(axis, radians)
}

export function vec2three(vec) {
  return new THREE.Vector3().fromArray(vec)
}
