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
