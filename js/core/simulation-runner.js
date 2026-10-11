const runningSolves = new WeakMap()

const faceKeys = {
  static: ['fixedFaces', 'loadFaces'],
  thermal: ['coldFaces', 'hotFaces'],
  fluid: ['inletFaces', 'outletFaces'],
}


export function cancelSimulation(simulation) {
  const pending = runningSolves.get(simulation)
  if(!pending) return

  runningSolves.delete(simulation)
  pending.worker.terminate()
  pending.finish()
}


export async function solveSimulation(simulation, tree, onProgress) {
  cancelSimulation(simulation)

  const problem = simulation.resolve(tree)
  simulation.result = null
  simulation.revision++
  if(problem) return typeof problem == 'string' ? problem : problem.message

  const groups = faceKeys[simulation.mode].map(key => simulation[key])
  const solid = groups[0][0].item.solid
  const componentId = groups[0][0].componentId
  const solidId = solid.id

  onProgress(0)

  const worker = new Worker(new URL('./simulation-worker.js', import.meta.url), { type: 'module' })

  const result = await new Promise(finish => {
    runningSolves.set(simulation, { worker, finish })

    worker.onmessage = ({ data }) => {
      if(data.type == 'progress') onProgress(data.progress)
      else finish(data.result)
    }

    worker.onerror = () => finish('The simulation solver failed; check the geometry and settings')

    const faces = solid.faces().map(face => ({
      positions: Float64Array.from(face.tesselate().positions),
      planar: simulation.mode == 'static' && !!face.getPlane(),
      groups: groups.map(references => references.some(reference => reference.item === face)),
    }))

    const material = simulation.mode == 'static' ? tree.findChild(componentId).getMaterial() : null
    const settings = Object.fromEntries([
      'quality', 'forceX', 'forceY', 'forceZ', 'coldTemperature', 'hotTemperature',
      'resolution', 'density', 'viscosity', 'inletSpeed', 'outletPressure',
    ].map(key => [key, simulation[key]]))

    if(material) settings.material = { elasticModulus: material.elasticModulus, poissonRatio: material.poissonRatio }

    worker.postMessage({ mode: simulation.mode, faces, settings }, faces.map(face => face.positions.buffer))
  })

  if(runningSolves.get(simulation)?.worker !== worker) return

  cancelSimulation(simulation)

  if(typeof result == 'string') return result
  if(!result) return 'The selected faces do not reach the simulation grid'

  result.componentId = componentId
  result.solidId = solidId
  simulation.result = result
  simulation.revision++
}
