import { readFile } from 'node:fs/promises'
import initShapex from '../js/shapexJS/index.js'
import { ShapexBackend } from '../js/kernels/shapex.js'
import { initializeOcc } from './occ-runtime.mjs'
import { registerTopologyTests } from './topology-contract.mjs'

await initializeOcc()
const api = await initShapex({ wasm: await readFile(new URL('../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)) })
window.kernel.modeling = new ShapexBackend(api)
await registerTopologyTests('Shapex')
