import assert from 'node:assert/strict'
import { execSync } from 'node:child_process'
import { readFile, readdir } from 'node:fs/promises'
import { delimiter, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { initialize, loadDocument } from './runtime.mjs'

// Explicitly invoked only; these known failures do not enter npm test.
// Default: report modeling failures and exit 1, as a normal test run would.
// --verify-failures: confirm each document has exactly one final failing feature.
const verifyFailures = process.argv.includes('--verify-failures')
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
for(const file of files) {
  const source = await readFile(new URL(file, import.meta.url), 'utf8')
  const document = loadDocument(source)
  const { features, marker } = document.timeline
  assert.equal(marker, features.length, `${file}: timeline must include the final feature`)
  const errors = features.filter(feature => feature.error?.type == 'error')
  console.log(`\n${file}`)
  for(const feature of features) {
    assert.ok(feature.isComplete(), `${file}: incomplete ${feature.title} ${feature.id}`)
    console.log(`  ${feature.error?.type == 'error' ? 'FAIL' : 'OK'} ${feature.title} ${feature.id}`)
    if(feature.error) console.log(`    ${feature.error.msg}`)
  }
  if(verifyFailures) {
    assert.deepEqual(errors.map(feature => feature.id), [features.at(-1).id],
      `${file}: expected only the final feature to fail; review newly passing cases`)
    assert.match(errors[0].error.msg, /^Shapex /, `${file}: failure must originate in modeling`)
    // A saved topology/profile reference failure is not a kernel test case.
    features.at(-1).updateReferences(document.timeline.tree(features.length - 1))
    const final = features.at(-1)
    assert.equal(final.error, undefined, `${file}: every saved reference must resolve, without partial loss`)
    if(final.constructor.name == 'ShellFeature') {
      for(const ref of final.faces() || []) {
        assert.ok(!ref.getItem().connectedFaces().some(face => face.id.includes('/fillet/')),
          `${file}: shell openings bordering fillets are excluded from this library`)
      }
    }
    if(['FilletFeature', 'ChamferFeature'].includes(final.constructor.name)) {
      for(const ref of final.edges()) {
        const edge = ref.getItem()
        assert.ok(edge.isSemantic(), `${file}: blend selection must not be a seam`)
        assert.equal(edge.connectedFaces().length, 2, `${file}: blend edge must join two faces`)
      }
    }
    if(file.includes('multiple-solids')) {
      const final = features.at(-1)
      assert.equal(final.constructor.name, 'ShellFeature', `${file}: expected one shell operation`)
      const selectedSolids = new Set(final.faces().map(ref => ref.getItem().solid.id))
      assert.ok(selectedSolids.size >= 2, `${file}: shell openings must resolve on multiple solids`)
    }
  }
  failures += errors.length
}
console.log(`\n${files.length} documents, ${failures} failing features.`)
if(!verifyFailures && failures) process.exitCode = 1
