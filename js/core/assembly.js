import * as THREE from 'three'


const IDENTITY = () => new THREE.Matrix4()
const EPSILON = 1e-8
const RELAXATION = 0.55

export function localTransform(component) {
  return (component.designTransform || IDENTITY()).clone()
    .multiply(component.transform || IDENTITY())
}

export function worldTransform(component) {
  const local = localTransform(component)
  return component.parent ? worldTransform(component.parent).multiply(local) : local
}

export function baselineWorldTransform(component) {
  const local = (component.designTransform || IDENTITY()).clone()
  return component.parent ? baselineWorldTransform(component.parent).multiply(local) : local
}

export function setWorldTransform(component, world) {
  if(!component.parent) return
  const parentWorld = worldTransform(component.parent)
  const local = parentWorld.clone().invert().multiply(world)
  const baseline = component.designTransform || IDENTITY()
  const pose = baseline.clone().invert().multiply(local)
  component.transform = matrixAlmostIdentity(pose) ? null : pose
}

export function setBaselineWorldTransform(component, world) {
  if(!component.parent) return
  const parentWorld = baselineWorldTransform(component.parent)
  component.designTransform = parentWorld.clone().invert().multiply(world)
  component.transform = null
}

export function referenceComponentId(reference) {
  let current = reference
  while(current) {
    if(current.componentId) return current.componentId
    current = current.item
  }
}

export function jointFrame(type, item) {
  if(type == 'fix') return IDENTITY()
  if(item instanceof THREE.Matrix4) return item.clone()
  if(type == 'axis') return item && item.getAxis && item.getAxis()
  if(type == 'coplanar') return item && item.getPlane && item.getPlane()
  if(type == 'ball') return item && item.getPoint && item.getPoint()
}

// Places component B on the constraint while retaining every unconstrained
// degree of freedom (axis slide/spin, plane slide/spin, and ball rotation).
export function alignJointWorld(type, worldA, frameA, worldB, frameB) {
  const attachmentA = worldA.clone().multiply(frameA)
  let result = worldB.clone()
  let attachmentB = result.clone().multiply(frameB)

  if(type == 'axis' || type == 'coplanar') {
    const directionA = frameDirection(attachmentA)
    const directionB = frameDirection(attachmentB)
    result = rotateWorldAbout(result, framePosition(attachmentB), directionB, directionA, 1)
    attachmentB = result.clone().multiply(frameB)
  }

  const a = framePosition(attachmentA)
  const b = framePosition(attachmentB)
  let delta = a.clone().sub(b)
  if(type == 'axis') {
    const axis = frameDirection(attachmentA)
    delta.sub(axis.multiplyScalar(delta.dot(axis)))
  } else if(type == 'coplanar') {
    const normal = frameDirection(attachmentA)
    delta = normal.multiplyScalar(delta.dot(normal))
  }
  return translateWorld(result, delta)
}

export function solveAssembly(tree, movedComponent, desiredWorld, iterations = 64) {
  const joints = tree.assemblyJoints || []
  if(!movedComponent || !movedComponent.parent || !joints.length) {
    if(movedComponent) setWorldTransform(movedComponent, desiredWorld)
    return
  }

  const fixed = new Set(
    joints.filter(joint => joint.type == 'fix').map(joint => joint.componentA)
  )
  const pinned = movedComponent.id
  if(fixed.has(pinned)) {
    const anchor = joints.find(joint => joint.type == 'fix' && joint.componentA == pinned)
    if(anchor) setWorldTransform(movedComponent, anchor.fixedWorld)
    return
  }
  const mobility = component => {
    if(!component || !component.parent || fixed.has(component.id) || component.id == pinned) return 0
    return 1
  }

  setWorldTransform(movedComponent, desiredWorld)
  for(let i = 0; i < iterations; i++) {
    for(const joint of joints) {
      if(joint.type == 'fix') {
        const component = tree.findChild(joint.componentA)
        if(component && component.id != pinned) setWorldTransform(component, joint.fixedWorld)
        continue
      }

      const a = tree.findChild(joint.componentA)
      const b = tree.findChild(joint.componentB)
      if(!a || !b) continue
      solvePair(joint, a, b, mobility(a), mobility(b))
    }
    // Drag intent and anchors are hard constraints.
    if(!fixed.has(pinned)) setWorldTransform(movedComponent, desiredWorld)
  }
}

function solvePair(joint, componentA, componentB, mobilityA, mobilityB) {
  const total = mobilityA + mobilityB
  if(total == 0) return
  const weightA = mobilityA / total
  const weightB = mobilityB / total

  let worldA = worldTransform(componentA)
  let worldB = worldTransform(componentB)
  let attachmentA = worldA.clone().multiply(joint.frameA)
  let attachmentB = worldB.clone().multiply(joint.frameB)

  if(joint.type == 'ball') {
    if(mobilityA) {
      worldA = rotateAttachmentToward(worldA, framePosition(attachmentA), framePosition(attachmentB), weightA * RELAXATION)
      setWorldTransform(componentA, worldA)
    }
    if(mobilityB) {
      worldB = rotateAttachmentToward(worldB, framePosition(attachmentB), framePosition(attachmentA), weightB * RELAXATION)
      setWorldTransform(componentB, worldB)
    }
    attachmentA = worldTransform(componentA).multiply(joint.frameA)
    attachmentB = worldTransform(componentB).multiply(joint.frameB)
  }

  if(joint.type == 'axis' || joint.type == 'coplanar') {
    const directionA = frameDirection(attachmentA)
    const directionB = frameDirection(attachmentB)
    if(mobilityA) {
      worldA = rotateWorldAbout(worldA, framePosition(attachmentA), directionA, directionB, weightA * RELAXATION)
      setWorldTransform(componentA, worldA)
    }
    if(mobilityB) {
      worldB = rotateWorldAbout(worldB, framePosition(attachmentB), directionB, directionA, weightB * RELAXATION)
      setWorldTransform(componentB, worldB)
    }
    attachmentA = worldTransform(componentA).multiply(joint.frameA)
    attachmentB = worldTransform(componentB).multiply(joint.frameB)
  }

  const pointA = framePosition(attachmentA)
  const pointB = framePosition(attachmentB)
  let error = pointA.clone().sub(pointB)
  if(joint.type == 'axis') {
    const axis = frameDirection(attachmentA)
    error.sub(axis.multiplyScalar(error.dot(axis)))
  } else if(joint.type == 'coplanar') {
    const normal = frameDirection(attachmentA)
    error = normal.multiplyScalar(error.dot(normal))
  }

  if(mobilityA) setWorldTransform(componentA, translateWorld(worldTransform(componentA), error.clone().multiplyScalar(-weightA * RELAXATION)))
  if(mobilityB) setWorldTransform(componentB, translateWorld(worldTransform(componentB), error.clone().multiplyScalar(weightB * RELAXATION)))
}

function rotateWorldAbout(world, pivot, from, to, weight) {
  if(from.dot(to) < 0) to = to.clone().negate()
  const full = new THREE.Quaternion().setFromUnitVectors(from, to)
  const rotation = new THREE.Quaternion().slerp(full, weight)
  if(Math.abs(rotation.w - 1) < EPSILON) return world
  return new THREE.Matrix4().makeTranslation(pivot)
    .multiply(new THREE.Matrix4().makeRotationFromQuaternion(rotation))
    .multiply(new THREE.Matrix4().makeTranslation(pivot.clone().negate()))
    .multiply(world)
}

function rotateAttachmentToward(world, attachment, target, weight) {
  const origin = framePosition(world)
  const from = attachment.clone().sub(origin)
  const to = target.clone().sub(origin)
  if(from.lengthSq() < EPSILON || to.lengthSq() < EPSILON) return world
  return rotateWorldAbout(world, origin, from.normalize(), to.normalize(), weight)
}

function translateWorld(world, delta) {
  return new THREE.Matrix4().makeTranslation(delta).multiply(world)
}

function framePosition(frame) {
  return new THREE.Vector3().setFromMatrixPosition(frame)
}

function frameDirection(frame) {
  return new THREE.Vector3().setFromMatrixColumn(frame, 2).normalize()
}

function matrixAlmostIdentity(matrix) {
  const identity = IDENTITY().elements
  return matrix.elements.every((value, index) => Math.abs(value - identity[index]) < EPSILON)
}
