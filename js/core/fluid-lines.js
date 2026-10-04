import * as THREE from 'three'


export function traceFlowLines(result) {
  const lines = []
  const seedCount = Math.min(64, result.inletPositions.length / 3)
  const distance = result.step * 0.25
  const minimumSpeed = result.maxSpeed * 0.001
  const trace = (seed, direction) => {
    const samples = []
    let position = seed.clone()
    for(let iteration = 0; iteration < 400; iteration++) {
      const velocity = velocityAt(result, position)
      if(!velocity || velocity.length() <= minimumSpeed) break
      samples.push({ position, speed: velocity.length() })
      // Midpoint integration follows the interpolated field through bends.
      const midpoint = position.clone().addScaledVector(velocity.normalize(), direction * distance / 2)
      const middleVelocity = velocityAt(result, midpoint)
      if(!middleVelocity || middleVelocity.length() <= minimumSpeed) break
      position = position.clone().addScaledVector(middleVelocity.normalize(), direction * distance)
    }
    return samples
  }
  for(let seed = 0; seed < seedCount; seed++) {
    const index = Math.floor(seed * (result.inletPositions.length / 3) / seedCount)
    const position = new THREE.Vector3().fromArray(result.inletPositions, index * 3)
    const backward = trace(position, -1).reverse()
    const forward = trace(position, 1)
    const samples = backward.slice(0, -1).concat(forward)
    if(samples.length > 1) lines.push(samples)
  }
  return lines
}


function velocityAt(result, position) {
  const x = (position.x - result.min[0]) / result.step
  const y = (position.y - result.min[1]) / result.step
  const z = (position.z - result.min[2]) / result.step
  const i = Math.floor(x), j = Math.floor(y), k = Math.floor(z)
  // Use the actual containing cell. Nearest-cell probes would cross walls.
  if(i < 0 || j < 0 || k < 0 || i >= result.nx || j >= result.ny || k >= result.nz) return
  if(result.grid[i + result.nx * (j + result.ny * k)] < 0) return
  const left = Math.floor(x - 0.5), bottom = Math.floor(y - 0.5), back = Math.floor(z - 0.5)
  const fx = x - 0.5 - left, fy = y - 0.5 - bottom, fz = z - 0.5 - back
  const velocity = new THREE.Vector3()
  let weightSum = 0
  for(let dz = 0; dz <= 1; dz++) {
    for(let dy = 0; dy <= 1; dy++) {
      for(let dx = 0; dx <= 1; dx++) {
        const a = left + dx, b = bottom + dy, c = back + dz
        if(a < 0 || b < 0 || c < 0 || a >= result.nx || b >= result.ny || c >= result.nz) continue
        const node = result.grid[a + result.nx * (b + result.ny * c)]
        if(node < 0) continue
        const weight = (dx ? fx : 1 - fx) * (dy ? fy : 1 - fy) * (dz ? fz : 1 - fz)
        velocity.x += result.velocities[node * 3] * weight
        velocity.y += result.velocities[node * 3 + 1] * weight
        velocity.z += result.velocities[node * 3 + 2] * weight
        weightSum += weight
      }
    }
  }
  return velocity.divideScalar(weightSum)
}
