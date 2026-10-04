import { solveVoxelFlow } from './fluid-solver.js'

self.onmessage = ({ data }) => {
  const faces = data.faces.map(face => ({
    type: face.type,
    tesselate: () => ({ positions: face.positions }),
  }))
  const result = solveVoxelFlow(
    { faces: () => faces },
    faces.filter(face => face.type == 'inlet'),
    faces.filter(face => face.type == 'outlet'),
    data.settings,
  )
  if(typeof result == 'string') self.postMessage(result)
  else self.postMessage(result, [result.grid.buffer, result.velocities.buffer, result.pressures.buffer, result.inletPositions.buffer])
}
