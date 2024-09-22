// import * as THREE from 'three'
import { isProxy, toRaw } from 'vue'

export async function loadFile(filetype, path) {
  if(window.ipc) {
    return loadFileElectron(filetype, path)
  } else {
    return loadFileWeb(filetype)
  }
}

export async function saveFile(data, filetype, path, title) {
  if(window.ipc) {
    return saveFileElectron(data, filetype, path)
  } else {
    saveFileWeb(data, filetype, path || title)
  }
}

// Electron
async function loadFileElectron(filetype, path) {
  path = path || await window.ipc.invoke('get-load-path', filetype)
  if(!path) throw 'canceled'
  const data = await window.ipc.invoke('load-file', path)
  return {data, path}
}

async function saveFileElectron(data, filetype, path) {
  path = path || await window.ipc.invoke('get-save-path', filetype)
  if(!path) throw 'canceled'
  await window.ipc.invoke('save-file', path, data)
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

export function arrayRange(start, end, step=1) {
  return Array.from(
    { length: (end - start) / step + 1 },
    (_, i) => start + i * step
  )
}

export function shallowEqual(a, b) {
  const keys1 = Object.keys(a)
  const keys2 = Object.keys(b)
  if(keys1.length !== keys2.length) return false
  for(let key of keys1) {
    if(a[key] !== b[key]) return false
  }
  return true
}

export function vueEqual(a, b) {
  return vueRaw(a) == vueRaw(b)
}

export function vueRaw(obj) {
  if(isProxy(obj)){
    obj = toRaw(obj)
  }
  return obj
}

export function vueIndexOf(array, item) {
  for(let i = 0; i < array.length; i++) {
    if(vueEqual(array[i], item)) return i
  }
  return -1
}

// export function vecToThree(array) {
//   return new THREE.Vector3().fromArray(array)
// }

// export function matrix2three(obj) {
//   const data = Object.values(obj).flatMap(v => Object.values(v))
//   return new THREE.Matrix4().fromArray(data)
// }

// export function matrixFromThree(m) {
//   const data = m.toArray()
//   return objectifyVec4([
//     objectifyVec4(data.slice(0, 4)),
//     objectifyVec4(data.slice(4, 8)),
//     objectifyVec4(data.slice(8, 12)),
//     objectifyVec4(data.slice(12, 16)),
//   ])
// }

// function objectifyVec4(array) {
//   return {
//     x: array[0],
//     y: array[1],
//     z: array[2],
//     w: array[3],
//   }
// }

// export function intersectSets(setA, setB) {
//   const intersection = new Set()
//   for (const elem of setB) {
//     if (setA.has(elem)) {
//       intersection.add(elem)
//     }
//   }
//   return intersection
// }
