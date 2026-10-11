import * as THREE from 'three'

import { solveElasticSolid } from './elasticity.js'
import { qualityLevels, solidGridSteps } from './static.js'
import { solveVoxelHeat } from './thermal.js'
import { solveVoxelFlow } from './fluid-solver.js'


self.onmessage = ({ data }) => {
  const faces = data.faces.map(face => ({
    groups: face.groups,
    getPlane: () => face.planar,
    tesselate: () => ({ positions: face.positions }),
  }))

  const solid = { faces: () => faces }
  const first = faces.filter(face => face.groups[0])
  const second = faces.filter(face => face.groups[1])
  const settings = data.settings
  const progress = update => self.postMessage({ type: 'progress', progress: update })

  let result

  if(data.mode == 'static') {
    const { resolution, maxCells, thinLayers } = qualityLevels[settings.quality]
    result = solveElasticSolid(solid, first, second,
      new THREE.Vector3(settings.forceX, settings.forceY, settings.forceZ),
      settings.material, resolution, maxCells, solidGridSteps(solid, thinLayers), progress)
  } else if(data.mode == 'thermal') {
    result = solveVoxelHeat(solid, first, second, settings.coldTemperature, settings.hotTemperature, progress)
  } else {
    result = solveVoxelFlow(solid, first, second, settings, progress)
  }

  const buffers = result && typeof result != 'string'
    ? Object.values(result).filter(value => ArrayBuffer.isView(value)).map(value => value.buffer) : []

  self.postMessage({ type: 'result', result }, buffers)
}
