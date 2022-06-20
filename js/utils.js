import * as THREE from 'three'

const ipc = window.electron && window.electron.ipc


export async function loadFile(filetype, path) {
  if(window.electron) {
    return loadFileElectron(filetype, path)
  } else {
    return loadFileWeb(filetype)
  }
}

export async function saveFile(data, filetype, path, title) {
  if(window.electron) {
    return saveFileElectron(data, filetype, path)
  } else {
    saveFileWeb(data, filetype, path || title)
  }
}

// Electron
async function loadFileElectron(filetype, path) {
  path = path || await ipc.invoke('get-load-path', filetype)
  if(!path) throw 'canceled'
  const data = await ipc.invoke('load-file', path)
  return {data, path}
}

async function saveFileElectron(data, filetype, path) {
  const ipc = window.electron.ipc
  path = path || await ipc.invoke('get-save-path', filetype)
  if(!path) throw 'canceled'
  await ipc.invoke('save-file', path, data)
  return path
}

// Web
function loadFileWeb(filetype) {
  return new Promise((resolve) => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.alc'
    document.body.appendChild(input)
    input.addEventListener('change', (event) => {
      document.body.removeChild(input)
      const reader = new FileReader()
      reader.onload = (e) => resolve({data: e.target.result})
      reader.readAsText(event.target.files[0])
    }, false)
    input.click()
  })
}

function saveFileWeb(data, filetype, filename) {
  var blob = new Blob([data], {filetype})
  const a = document.createElement("a")
  const url = URL.createObjectURL(blob)
  a.href = url
  a.download = (filename || 'untitled').replace(new RegExp(' ', 'g'), '_') + '.' + filetype
  document.body.appendChild(a)
  a.click()
  setTimeout(function() {
    document.body.removeChild(a)
    window.URL.revokeObjectURL(url)
  }, 0)
}


export function rotationFromNormal(normal) {
  let up = THREE.Object3D.DefaultUp
  let xAxis
  if(Math.abs(normal.dot(up)) > 0.9999) {
    xAxis = new THREE.Vector3(1, 0, 0)
  } else {
    xAxis = new THREE.Vector3().crossVectors(up, normal).normalize()
  }
  const yAxis = new THREE.Vector3().crossVectors(normal, xAxis)
  const rot = new THREE.Matrix4().makeBasis(xAxis, yAxis, normal)
  return new THREE.Quaternion().setFromRotationMatrix(rot)
  // let radians = Math.acos(normal.dot(up))
  // return new THREE.Quaternion().setFromAxisAngle(xAxis, radians)
}

export function vec2three(vec) {
  return new THREE.Vector3().fromArray(vec)
}
