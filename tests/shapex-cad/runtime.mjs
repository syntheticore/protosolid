import { readFile } from 'node:fs/promises'
import { init_planegcs_module } from '@salusoft89/planegcs'
import { initializeSketchSolver } from '../../js/core/sketch-solver.js'
import { initializeShapex } from '../../js/core/shapex.js'
import { Timeline } from '../../js/core/timeline.js'
import Serialize from '../../js/core/serialize.js'

// Load the app's exact sketch geometry and solver implementation without Nuxt.
export async function initialize() {
  const gcs = await init_planegcs_module({
    wasmBinary: await readFile(new URL('../../node_modules/@salusoft89/planegcs/dist/planegcs_dist/planegcs.wasm', import.meta.url)),
  })
  await initializeShapex({
    wasm: await readFile(new URL('../../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)),
  })
  initializeSketchSolver(gcs)
  globalThis.window = {
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
