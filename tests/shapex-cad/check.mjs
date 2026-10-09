import assert from 'node:assert/strict'
import { execSync } from 'node:child_process'
import { readFile, readdir } from 'node:fs/promises'
import { delimiter, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { initialize, loadDocument } from './runtime.mjs'
import { ShellFeature } from '../../js/core/features.js'

// Rebuild from the current Rust source before loading the WASM package.
execSync('npm run build-shapex', {
  cwd: fileURLToPath(new URL('../..', import.meta.url)),
  stdio: 'inherit',
  env: { ...process.env, PATH: dirname(process.execPath) + delimiter + process.env.PATH },
})
await initialize()
const files = (await readdir(new URL('.', import.meta.url)))
  .filter(file => file.endsWith('.cad')).sort()
assert.ok(files.length, 'No CAD cases found')
let failures = 0
let regressions = 0
function checkDocument(file, source) {
  const document = loadDocument(source)
  const { features, marker } = document.timeline
  assert.equal(marker, features.length, `${file}: timeline must include the final feature`)
  const errors = features.filter(feature => feature.error)
  failures += errors.length
  console.log(`\n${file}`)
  for(const feature of features) {
    assert.ok(feature.isComplete(), `${file}: incomplete ${feature.title} ${feature.id}`)
    console.log(`  ${feature.error ? 'FAIL' : 'OK'} ${feature.title} ${feature.id}`)
    if(feature.error) console.log(`    ${feature.error.msg}`)
    if(feature.constructor.name == 'LoftFeature' && !feature.error) {
      const faces = feature.previewBody.faces()
      const square = file.includes('square-loft')
      assert.equal(faces.length, square ? 6 : 3, `${file}: loft must retain its section topology`)
      assert.equal(faces.filter(face => face.getPlane()).length, file.includes('tapered-square') ? 6 : 2,
        `${file}: loft must retain exact planar supports`)
    }
  }
  const final = features.at(-1)
  assert.equal(errors.length, 0, `${file}: every feature must build without errors`)
  final.updateReferences(document.timeline.tree(features.length - 1))
  assert.equal(final.error, undefined, `${file}: every saved reference must resolve without partial loss`)
  if(file.startsWith('021-')) {
    for(const feature of features.slice(1)) {
      const body = document.top(feature).compound
      const volume = body.volume()
      const sides = body.faces().filter(face => {
        const plane = face.getPlane()
        return plane && Math.abs(plane.elements[10]) > 0.99
      })
      assert.ok(sides.length >= 2, `${file}: large planar side faces must exist`)
      for(const face of sides) {
        const halves = [true, false].map(side => body.halfCut(face.getPlane(), side))
        assert.ok(Math.abs(halves[0].volume() + halves[1].volume() - volume) < volume * 1e-6,
          `${file}: ${feature.title} section must preserve total volume`)
        for(const half of halves) {
          for(const cutFace of half.faces()) window.kernel.modeling.mesh(cutFace.geom())
        }
      }
    }
    console.log('  OK both section sides at every large planar face through the feature sequence')
  }
  if(['FilletFeature', 'ChamferFeature'].includes(final.constructor.name)) {
    for(const ref of final.edges()) {
      const edge = ref.getItem()
      assert.ok(edge.isSemantic(), `${file}: blend selection must not be a seam`)
      assert.equal(edge.connectedFaces().length, 2, `${file}: blend edge must join two faces`)
    }
  }
  const compound = document.top().findChild(final.componentId).compound
  if(file.startsWith('019-')) {
    const edges = compound.solids().flatMap(solid => solid.edges())
    assert.equal(edges.length, 24, `${file}: chamfer must retain all boundary edges`)
    assert.ok(edges.every(edge => edge.isSemantic()), `${file}: chamfer boundaries must be visible`)
  }
  if(file.startsWith('020-')) {
    const verifyShell = (shell, source) => {
      assert.equal(shell.solids().length, 1, `${file}: follow-up shell must produce one solid`)
      assert.ok(shell.volume() > 0 && shell.volume() < source.volume(),
        `${file}: follow-up shell must retain a positive wall volume`)
      for(const face of shell.faces()) window.kernel.modeling.mesh(face.geom())
    }
    for(const thickness of [1, 2]) verifyShell(compound.shell([], -thickness), compound)
    const opening = compound.faces().find(face => {
      const plane = face.getPlane()
      return plane && Math.abs(plane.elements[9]) > 0.99
    })
    assert.ok(opening, `${file}: planar radial opening must exist`)
    const reference = opening.faceReference()
    const shellFeature = new ShellFeature(document)
    shellFeature.faces = () => [reference]
    document.timeline.insertFeature(shellFeature, false)
    const shellIndex = document.timeline.features.indexOf(shellFeature)
    for(const thickness of [1, 2]) {
      shellFeature.thickness = thickness
      document.timeline.regenerate(shellIndex, document.timeline.marker)
      assert.ok(!shellFeature.error, `${file}: open shell ${thickness} mm: ${shellFeature.error?.msg}`)
      verifyShell(document.top().compound, compound)
    }
    shellFeature.thickness = 1
    const base = document.top(document.timeline.previousFeature(final)).compound
    const rims = base.edges().filter(edge => edge.vertices()[0].distanceTo(edge.vertices()[1]) > 10)
    assert.equal(rims.length, 4, `${file}: four circular rims must exist`)
    const expanded = [...final.edges(), ...rims.map(edge => edge.edgeReference())]
    final.edges = () => expanded
    document.timeline.regenerate(document.timeline.features.indexOf(final), document.timeline.marker)
    assert.ok(!final.error, `${file}: expanded fillet: ${final.error?.msg}`)
    assert.equal(reference.getItem()?.id, opening.id, `${file}: opening identity must survive the fillet edit`)
    assert.ok(reference.getItem()?.getPlane(), `${file}: opening must remain planar`)
    for(const thickness of [1, 2]) {
      shellFeature.thickness = thickness
      document.timeline.regenerate(shellIndex, document.timeline.marker)
      assert.ok(!shellFeature.error, `${file}: edited fillet shell ${thickness} mm: ${shellFeature.error?.msg}`)
      verifyShell(document.top().compound, document.top(final).compound)
    }
    console.log('  OK 1 and 2 mm shells, face tessellation and opening reference after fillet edit')
  }
}
for(const file of files) {
  try {
    checkDocument(file, await readFile(new URL(file, import.meta.url), 'utf8'))
  } catch(error) {
    regressions++
    console.error(`  FAIL ${file}: ${error.message || error.msg || error}`)
  }
}
console.log(`\n${files.length} documents, ${failures} failing features, ${regressions} failed document checks.`)
if(regressions || failures) process.exitCode = 1
