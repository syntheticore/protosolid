import * as THREE from 'three'

// Curvature and unit principal normal, pointing to the center of curvature,
// at evenly spaced parameters of a 2D or 3D trimmed curve
export function curvatureSamples(curve, transform, count) {
  const range = curve.range()
  return Array.from({ length: count }, (_, i) => {
    const evaluation = curve.evaluate(range.start + range.span() * i / (count - 1))
    const [point, d1, d2] = [evaluation.point, evaluation.firstDerivative, evaluation.secondDerivative]
      .map(value => new THREE.Vector3(value.x, value.y, value.z || 0))
    const speed = d1.length()
    const normal = d2.clone().addScaledVector(d1, -d1.dot(d2) / speed ** 2).normalize()
    return {
      point: point.applyMatrix4(transform),
      normal: normal.transformDirection(transform),
      kappa: new THREE.Vector3().crossVectors(d1, d2).length() / speed ** 3,
    }
  })
}

// Tooth length per unit curvature that gives the strongest curvature
// teeth a fifth the size of all samples together
export function toothScale(samples, scale) {
  const maxKappa = Math.max(...samples.map(sample => sample.kappa))
  const size = new THREE.Box3().setFromPoints(samples.map(sample => sample.point)).getSize(new THREE.Vector3()).length()
  return scale * 0.2 * size / maxKappa
}

// Comb lines of sample lists: teeth pointing away from the center of
// curvature, and envelopes connecting their tips
export function combLines(combs, scale) {
  const tips = combs.map(samples => samples.map(sample =>
    sample.point.clone().addScaledVector(sample.normal, -sample.kappa * scale)
  ))
  return {
    teeth: combs.flatMap((samples, i) => samples.map((sample, j) => [sample.point.toArray(), tips[i][j].toArray()])),
    envelopes: tips.map(points => points.map(point => point.toArray())),
  }
}
