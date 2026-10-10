import { initializeOcc } from './occ-runtime.mjs'
import { registerTopologyTests } from './topology-contract.mjs'

await initializeOcc()
await registerTopologyTests('OCC')
