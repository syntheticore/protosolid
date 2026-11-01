import assert from 'node:assert/strict'
import { execSync } from 'node:child_process'
import { readFile, readdir } from 'node:fs/promises'
import { delimiter, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { initialize, loadDocument } from './runtime.mjs'

// Rebuild from the current Rust source before loading the WASM package.
execSync('npm run build-shapex', {
  cwd: fileURLToPath(new URL('../..', import.meta.url)),
  stdio: 'inherit',
  env: { ...process.env, PATH: dirname(process.execPath) + delimiter + process.env.PATH },
})
await initialize()
// Checks this directory, or a subdirectory such as `failing` given as argument
const directory = new URL(process.argv[2] ? `${process.argv[2]}/` : '.', import.meta.url)
const files = (await readdir(directory))
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
  }
  const final = features.at(-1)
  assert.equal(errors.length, 0, `${file}: every feature must build without errors`)
  final.updateReferences(document.timeline.tree(features.length - 1))
  assert.equal(final.error, undefined, `${file}: every saved reference must resolve without partial loss`)
  if(['FilletFeature', 'ChamferFeature'].includes(final.constructor.name)) {
    for(const ref of final.edges()) {
      const edge = ref.getItem()
      assert.ok(edge.isSemantic(), `${file}: blend selection must not be a seam`)
      assert.equal(edge.connectedFaces().length, 2, `${file}: blend edge must join two faces`)
    }
  }
}
for(const file of files) {
  try {
    checkDocument(file, await readFile(new URL(file, directory), 'utf8'))
  } catch(error) {
    regressions++
    console.error(`  FAIL ${file}: ${error.message || error.msg || error}`)
  }
}
console.log(`\n${files.length} documents, ${failures} failing features, ${regressions} failed document checks.`)
if(regressions || failures) process.exitCode = 1
