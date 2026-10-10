import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { OccBackend } from '../js/kernels/occ.js'

// Exercise the shipped OCC kernel and production geometry classes without
// building or launching the application.
export async function initializeOcc() {
  const url = new URL('../assets/oc/oc.js', import.meta.url)
  const source = await readFile(url, 'utf8')
  const context = vm.createContext({
    require: createRequire(url), __dirname: fileURLToPath(new URL('.', url)),
    __filename: fileURLToPath(url), process, console, Buffer, WebAssembly,
    TextDecoder, TextEncoder, setTimeout, clearTimeout, setInterval, clearInterval,
  })
  const init = vm.runInContext(source.replace('export default Module;', 'Module;'), context)
  const oc = await init({ wasmBinary: await readFile(new URL('oc.wasm', url)) })
  globalThis.window = { oc: { oc }, kernel: { modeling: new OccBackend(oc) }, addEventListener() {} }
  return oc
}
