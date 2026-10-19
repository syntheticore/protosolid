import fs from 'node:fs'
import { initializeShapex } from '../js/core/shapex.js'
import Serialize from '../js/core/serialize.js'
import { lastId, setLastId } from '../js/core/id.js'

// Use the application's WASM transport and feature code, but retain the saved
// sketch coordinates. This isolates Shapex from solving, UI and asset loading.
// Usage: node [--cpu-prof] scripts/profile-shapex-scene.mjs path/to/scene.cad
if(!process.argv[2]) {
  console.error('Usage: node [--cpu-prof] scripts/profile-shapex-scene.mjs scene.cad')
  process.exit(1)
}

try {
  await profileScene(process.argv[2])
} catch(error) {
  console.error(error?.message || error)
  process.exitCode = 1
}

async function profileScene(scenePath) {
  globalThis.window = { addEventListener() {}, localStorage: { getItem() { return null } } }
  const { default: Document } = await import('../js/core/document.js')
  const { Sketch } = await import('../js/core/sketch.js')
  Sketch.prototype.solve = function() {}
  await initializeShapex({ wasm: fs.readFileSync(new URL('../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)) })

  const doc = new Document()
  const dump = Serialize.parse(fs.readFileSync(scenePath, 'utf8'), { document: doc, sketches: {} })
  setLastId(dump.lastId > lastId ? dump.lastId : lastId)
  doc.timeline = dump.timeline
  const measurements = []
  for(const feature of doc.timeline.features) {
    const execute = feature.execute
    feature.execute = function(...args) {
      const start = performance.now()
      try { return execute.apply(this, args) }
      finally {
        measurements.push({ feature: this.title, id: this.id, ms: Math.round(performance.now() - start), error: this.error })
      }
    }
  }
  const start = performance.now()
  doc.timeline.evaluate()
  const geometryMs = Math.round(performance.now() - start)
  console.table(measurements)
  const failures = measurements.filter(item => item.error?.type == 'error')
  if(failures.length) throw new Error(`Scene evaluation failed: ${JSON.stringify(failures)}`)
  const shape = doc.timeline.tree().compound
  const meshStart = performance.now()
  let triangles = 0
  for(const face of shape.faces()) triangles += face.tesselate().positions.length / 9
  console.log(JSON.stringify({
    scene: scenePath,
    geometryMs,
    displayMeshMs: Math.round(performance.now() - meshStart),
    triangles,
    volume: shape.geom().volume(),
    area: shape.geom().surfaceArea(),
    faces: shape.faces().length,
    edges: shape.edges().length,
  }, null, 2))
}
