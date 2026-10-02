import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { GcsWrapper, init_planegcs_module } from '@salusoft89/planegcs'
import initShapex from '../../js/shapexJS/index.js'
import { ShapexBackend } from '../../js/kernels/shapex.js'
import { Timeline } from '../../js/core/timeline.js'
import Serialize from '../../js/core/serialize.js'

// Load the app's exact sketch geometry and solver implementation without Nuxt.
// OpenCascade is used only for 2D sketches; all modeling uses Shapex.
export async function initialize() {
  const ocURL = new URL('../../assets/oc/oc.js', import.meta.url)
  const source = await readFile(ocURL, 'utf8')
  const context = vm.createContext({
    require: createRequire(ocURL),
    __dirname: fileURLToPath(new URL('.', ocURL)),
    __filename: fileURLToPath(ocURL),
    process, console, Buffer, WebAssembly, TextDecoder, TextEncoder,
    setTimeout, clearTimeout, setInterval, clearInterval,
  })
  const initOC = vm.runInContext(source.replace('export default Module;', 'Module;'), context)
  const oc = await initOC({ wasmBinary: await readFile(new URL('oc.wasm', ocURL)) })
  const gcs = await init_planegcs_module({
    wasmBinary: await readFile(new URL('../../node_modules/@salusoft89/planegcs/dist/planegcs_dist/planegcs.wasm', import.meta.url)),
  })
  const solverSource = await readFile(new URL('../../composables/use-oc.js', import.meta.url), 'utf8')
  const useOC = new Function('ocp', 'gcsp', 'GcsWrapper',
    solverSource.slice(solverSource.indexOf('export async function useOC()'))
      .replace('export async function', 'async function') + '\nreturn useOC()')
  const api = await initShapex({
    wasm: await readFile(new URL('../../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)),
  })
  globalThis.window = {
    oc: await useOC(Promise.resolve(oc), Promise.resolve(gcs), GcsWrapper),
    kernel: { modeling: new ShapexBackend(api) },
    bus: { emit: (...args) => console.error('Sketch notification:', ...args) },
  }
}

export function newDocument() {
  const document = { timeline: new Timeline() }
  document.top = at => document.timeline.tree(at)
  document.activeComponent = document.top()
  return document
}

export function loadDocument(source) {
  const document = newDocument()
  const dump = Serialize.parse(source, { document, sketches: {} })
  document.timeline = dump.timeline
  // The same feature execution, reference resolution, repair and face meshing
  // as opening a .cad in the app, using the synchronous timeline entry point.
  document.timeline.regenerate(0, document.timeline.marker)
  return document
}
