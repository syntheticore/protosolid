import * as THREE from 'three'

import { CentroidHelper, trackCentroidReads } from './helpers.js'
import { baselineWorldTransform } from './assembly.js'
import { getCenterOfMassTolerance } from './shapex.js'

// Passes re-running the features that read a centroid before giving up
const MAX_ITERATIONS = 20

// Moves the centroid estimates of a timeline to the center of mass of its
// final components. Features reading a centroid run again after each move,
// until every estimate is within tolerance.
export class CentroidSolver {
  constructor() {
    // Source ids of the components whose centroid each feature read, by feature id
    this.readers = new Map()
    // Remaining deviation by component id, where the centroid did not converge
    this.errors = {}
    // Last estimates and residuals of the running solve
    this.history = {}
  }

  // Executes a feature, noting the centroids it reads
  execute(feature, tree, execute) {
    this.readers.set(feature.id, trackCentroidReads(execute))
    // Components created by the feature start with their centroid
    syncCentroidHelpers(tree)
  }

  // The centroid helpers of every tree stand for the center of mass of the
  // final components. Moves the estimates to the actual centers of mass of
  // the final tree and returns the index of the first feature that read a
  // moved estimate, which needs to run again. Returns null once all
  // estimates are within tolerance, or without a final tree.
  update(trees, finalTree, features, iteration) {
    if(!finalTree) return null
    const tolerance = getCenterOfMassTolerance()
    if(iteration == 0) this.history = {}
    this.errors = {}
    let first = null
    let moved = false
    for(const { component, actual, deviation } of centroidDeviations(finalTree)) {
      if(deviation <= tolerance) continue
      const reader = this.firstReaderIndex(features, component.id)
      if(reader >= 0 && iteration >= MAX_ITERATIONS) {
        this.errors[component.id] = deviation
        continue
      }
      const estimate = component.creator.cogPosition
      const next = reader >= 0 && actual && estimate ?
        nextCentroidEstimate(this.history, component.id, new THREE.Vector3().copy(estimate), actual) :
        actual
      component.creator.cogPosition = next && { x: next.x, y: next.y, z: next.z }
      moved = true
      if(reader >= 0) first = first === null ? reader : Math.min(first, reader)
    }
    if(moved) this.sync(trees)
    return first
  }

  // Adds, moves or removes the centroid helpers of the given trees
  sync(trees) {
    trees.forEach(tree => syncCentroidHelpers(tree))
  }

  // First feature that read the centroid of a component
  firstReaderIndex(features, componentId) {
    return features.findIndex(feature => this.readers.get(feature.id)?.has(componentId))
  }
}

// Rebuilds a centroid helper when its estimate moved or it was removed
function syncCentroidHelpers(tree) {
  tree.getChildren().forEach(component => {
    const position = component.creator.cog ? component.creator.cogPosition : undefined
    const existing = component.helpers.find(helper => helper instanceof CentroidHelper)
    if(existing ? samePosition(existing.position, position) : position === undefined) return
    const helpers = component.helpers.filter(helper => !(helper instanceof CentroidHelper))
    if(position !== undefined) helpers.push(new CentroidHelper(component.id, position))
    component.helpers = helpers
  })
}

// Positions compare by value, as trees may hold reactive proxies of them
function samePosition(a, b) {
  if(!a || !b) return a === b
  return a.x === b.x && a.y === b.y && a.z === b.z
}

// Center of mass of a component with its children, in component space.
// Weighs by volume alone unless every body has a material.
export function centerOfMass(component) {
  const toLocal = baselineWorldTransform(component).invert()
  const byMass = new THREE.Vector3()
  const byVolume = new THREE.Vector3()
  let mass = 0
  let volume = 0
  let complete = true
  component.getChildren().forEach(owner => {
    const solids = owner.compound.solids()
    if(!solids.length) return
    const density = owner.getMaterial()?.density
    if(density == null) complete = false
    const transform = toLocal.clone().multiply(baselineWorldTransform(owner))
    solids.forEach(solid => {
      const solidVolume = solid.volume()
      const center = solid.center().applyMatrix4(transform)
      volume += solidVolume
      byVolume.addScaledVector(center, solidVolume)
      if(density == null) return
      mass += solidVolume * density
      byMass.addScaledVector(center, solidVolume * density)
    })
  })
  if(!volume) return null
  return complete && mass > 0 ? byMass.divideScalar(mass) : byVolume.divideScalar(volume)
}

// Distance between the estimated and the actual center of mass
// for each source component of the final tree that tracks its centroid
function centroidDeviations(tree) {
  return tree.getChildren()
    .filter(component => component.creator.cog && !component.instanceOf)
    .map(component => {
      const actual = centerOfMass(component)
      const estimate = component.creator.cogPosition
      const deviation = actual && estimate ? actual.distanceTo(estimate) :
        actual || estimate ? Infinity : 0
      return { component, actual, deviation }
    })
}

// Next estimate for a center of mass that moves with its own estimate,
// solving estimate = actual(estimate). The secant slope of the residual
// along the last step corrects plain substitution, which converges slowly
// or not at all where a feature reading the centroid moves a large share of
// the mass. History keeps the last estimate and residual by component id.
function nextCentroidEstimate(history, id, estimate, actual) {
  const residual = actual.clone().sub(estimate)
  const previous = history[id]
  history[id] = { estimate, residual }
  if(!previous) return actual
  const step = estimate.clone().sub(previous.estimate)
  const slope = residual.clone().sub(previous.residual).dot(step) / step.lengthSq()
  // Plain substitution, where the slope is unreliable
  if(!(slope < -0.1)) return actual
  return estimate.clone().addScaledVector(residual, Math.min(-1 / slope, 10))
}
