import assert from 'node:assert/strict'
import { execSync } from 'node:child_process'
import { readFile, readdir } from 'node:fs/promises'
import { delimiter, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { initialize, loadDocument } from '../runtime.mjs'

const verifyFailures = process.argv.includes('--verify-failures')
execSync('npm run build-shapex', {
  cwd: fileURLToPath(new URL('../../..', import.meta.url)),
  stdio: 'inherit',
  env: { ...process.env, PATH: dirname(process.execPath) + delimiter + process.env.PATH },
})
await initialize()
const files = (await readdir(new URL('.', import.meta.url)))
  .filter(file => file.endsWith('.cad')).sort()
assert.ok(files.length, 'No CAD cases found')
let failures = 0
let invalidDocuments = 0
for(const file of files) {
  try {
    const document = loadDocument(await readFile(new URL(file, import.meta.url), 'utf8'))
    const { features, marker } = document.timeline
    assert.equal(marker, features.length, `${file}: marker must include the final feature`)
    console.log(`\n${file}`)
    for(const feature of features) {
      assert.ok(feature.isComplete(), `${file}: incomplete ${feature.title}`)
      console.log(`  ${feature.error ? 'FAIL' : 'OK'} ${feature.title} ${feature.id}`)
      if(feature.error) console.log(`    ${feature.error.msg}`)
    }
    const final = features.at(-1)
    const errors = features.filter(feature => feature.error)
    failures += errors.length
    assert.ok(features.slice(0, -1).every(feature => !feature.error), `${file}: setup must build without warnings`)
    if(verifyFailures) {
      assert.deepEqual(errors.map(feature => feature.id), [final.id], `${file}: expected only the final feature to fail`)
      assert.equal(final.error.type, 'error', `${file}: final feature must fail, not warn`)
      assert.match(final.error.msg, /^Shapex /, `${file}: failure must originate in modeling`)
    }
    final.updateReferences(document.timeline.tree(features.length - 1))
    assert.equal(final.error, undefined, `${file}: every reference must resolve without partial loss`)
    if(['FilletFeature', 'ChamferFeature'].includes(final.constructor.name)) {
      for(const reference of final.edges()) {
        const edge = reference.getItem()
        assert.ok(edge.isSemantic(), `${file}: blend selection must not be a seam`)
        assert.equal(edge.connectedFaces().length, 2, `${file}: blend edge must join two faces`)
      }
    }
  } catch(error) {
    invalidDocuments++
    console.error(`  INVALID ${file}: ${error.message || error.msg || error}`)
  }
}
console.log(`\n${files.length} documents, ${failures} failing features, ${invalidDocuments} invalid document checks.`)
if(invalidDocuments || (failures && !verifyFailures)) process.exitCode = 1
