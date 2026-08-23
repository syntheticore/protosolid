import * as THREE from 'three'


const IDENTITY = () => new THREE.Matrix4()
const EPSILON = 1e-8
const SOLVER_TOLERANCE = 1e-4
const RELAXATION = 0.55

export function localTransform(component) {
  return (component.designTransform || IDENTITY()).clone()
    .multiply(component.transform || IDENTITY())
}

export function worldTransform(component) {
  const local = localTransform(component)
  return component.parent ? worldTransform(component.parent).multiply(local) : local
}

// Transform geometry expressed in source-local coordinates into target-local
// coordinates. Callers remain responsible for transforming their geometry
// type (BRep shape, point, plane, and so on).
export function relativeComponentTransform(source, target) {
  if(source == target) return IDENTITY()
  return worldTransform(target).invert().multiply(worldTransform(source))
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
export function alignJointWorld(type, worldA, frameA, worldB, frameB, lockSlide = false) {
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
  if(type == 'axis' && !lockSlide) {
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

  // Capture link coordinates before applying the requested drag. Angular
  // coordinates are unwrapped from this state across successive pointer moves.
  const motionLinks = tree.motionLinks || []
  motionLinks.forEach(link => {
    const linkedJoints = [link.jointA, link.jointB]
      .map(id => joints.find(joint => joint.id == id))
    if(linkedJoints.every(Boolean)) ensureMotionLinkState(tree, link, linkedJoints)
  })

  const fixed = new Set(
    joints.filter(joint => joint.type == 'fix').map(joint => joint.componentA)
  )

  const rigidGroup = connectedRigidGroup(tree, movedComponent, joints)
  const fixedGroupComponent = rigidGroup.find(component => fixed.has(component.id))
  if(fixedGroupComponent) {
    const fixedJoint = joints.find(joint =>
      joint.type == 'fix' && joint.componentA == fixedGroupComponent.id
    )
    if(fixedJoint) setWorldTransform(fixedGroupComponent, fixedJoint.fixedWorld)
    projectRigidGroup(tree, fixedGroupComponent, joints)
    return
  }
  if(rigidGroup.length > 1) {
    const transform = desiredWorld.clone().multiply(worldTransform(movedComponent).invert())
    transformWorldGroup(rigidGroup, transform)
  }

  if(fixed.has(movedComponent.id)) {
    const anchor = joints.find(joint => joint.type == 'fix' && joint.componentA == movedComponent.id)
    if(anchor) setWorldTransform(movedComponent, anchor.fixedWorld)
    return
  }
  let pathSolved
  const mobility = component => {
    if(!component || !component.parent || fixed.has(component.id)) return 0
    return !pathSolved && component.id == movedComponent.id ? 0 : 1
  }

  pathSolved = solveKinematicPath(tree, movedComponent, framePosition(desiredWorld), joints, fixed, iterations)
  if(!pathSolved) {
    setWorldTransform(movedComponent, desiredWorld)
  }

  const projectJoints = () => {
    for(const joint of joints) {
      if(joint.type == 'fix') {
        const component = tree.findChild(joint.componentA)
        if(component) setWorldTransform(component, joint.fixedWorld)
        continue
      }

      const a = tree.findChild(joint.componentA)
      const b = tree.findChild(joint.componentB)
      if(!a || !b) continue
      solvePair(joint, a, b, mobility(a), mobility(b))
    }
  }

  const activeLinks = motionLinks.map(link => {
    const jointA = joints.find(joint => joint.id == link.jointA)
    const jointB = joints.find(joint => joint.id == link.jointB)
    if(!jointA || !jointB) return
    return {
      link,
      joints: [jointA, jointB],
      // The unconstrained kinematic pass above reveals which linked degree of
      // freedom the drag actually drove. Lock that choice on first movement.
      driver: null,
    }
  }).filter(Boolean)

  for(let i = 0; i < iterations; i++) {
    projectJoints()
    if(!pathSolved) setWorldTransform(movedComponent, desiredWorld)
    const linkError = activeLinks.reduce((largest, active) =>
      Math.max(largest, projectMotionLink(tree, active, fixed)), 0
    )
    if(assemblyError(tree, joints) < SOLVER_TOLERANCE && linkError < SOLVER_TOLERANCE) break
  }

  activeLinks.forEach(active => updateMotionLinkState(tree, active.link, active.joints))
}

export function jointMotionOptions(joint) {
  if(!joint) return {}
  if(joint.type == 'axis') return {
    rotateZ: 'Rotate Z',
    ...(!joint.lockSlide ? { slideZ: 'Slide Z' } : {}),
  }
  if(joint.type == 'coplanar') return {
    slideX: 'Slide X',
    slideY: 'Slide Y',
    rotateZ: 'Rotate Z',
  }
  if(joint.type == 'ball') return {
    rotateX: 'Rotate X',
    rotateY: 'Rotate Y',
    rotateZ: 'Rotate Z',
  }
  return {}
}

export function captureMotionLinkStates(tree) {
  return new Map((tree.motionLinks || []).map(link => [
    link.id,
    link.state && link.state.map(state => ({ ...state })),
  ]))
}

export function restoreMotionLinkStates(tree, states) {
  const motionLinks = tree.motionLinks || []
  motionLinks.forEach(link => {
    const state = states.get(link.id)
    link.state = state && state.map(entry => ({ ...entry }))
  })
}

export function isRotationalMotion(motion) {
  return !!motion && motion.startsWith('rotate')
}

export function jointMotionValue(tree, joint, motion) {
  const componentA = joint && tree.findChild(joint.componentA)
  const componentB = joint && tree.findChild(joint.componentB)
  if(!componentA || !componentB) return 0
  const attachmentA = worldTransform(componentA).multiply(joint.frameA)
  const attachmentB = worldTransform(componentB).multiply(joint.frameB)
  const relative = attachmentA.clone().invert().multiply(attachmentB)
  if(motion.startsWith('slide')) {
    const axis = motionAxis(motion)
    return new THREE.Vector3().setFromMatrixPosition(relative).getComponent(axis)
  }
  const rotation = new THREE.Euler().setFromRotationMatrix(relative, 'XYZ')
  return [rotation.x, rotation.y, rotation.z][motionAxis(motion)]
}

function motionAxis(motion) {
  return { X: 0, Y: 1, Z: 2 }[motion.at(-1)]
}

function motionDelta(current, previous, rotational) {
  let delta = current - previous
  if(rotational) delta = THREE.MathUtils.euclideanModulo(delta + Math.PI, Math.PI * 2) - Math.PI
  return delta
}

function ensureMotionLinkState(tree, link, joints) {
  if(link.state?.length == 2) return link.state
  link.state = joints.map((joint, index) => {
    const raw = jointMotionValue(tree, joint, index ? link.motionB : link.motionA)
    return { raw, accumulated: 0 }
  })
  return link.state
}

function projectMotionLink(tree, active, fixed) {
  const { link, joints } = active
  let { driver } = active
  const motions = [link.motionA, link.motionB]
  const state = ensureMotionLinkState(tree, link, joints)
  if(driver === null) {
    const fractions = joints.map((joint, index) => {
      const raw = jointMotionValue(tree, joint, motions[index])
      const delta = motionDelta(raw, state[index].raw, isRotationalMotion(motions[index]))
      const travel = index ? link.travelB : link.travelA
      return Math.abs(delta / travel)
    })
    driver = fractions[1] > fractions[0] ? 1 : 0
    if(Math.max(...fractions) > EPSILON) active.driver = driver
  }
  const driven = 1 - driver
  const driverRaw = jointMotionValue(tree, joints[driver], motions[driver])
  const driverAccumulated = state[driver].accumulated + motionDelta(
    driverRaw,
    state[driver].raw,
    isRotationalMotion(motions[driver]),
  )
  const forwardRatio = link.travelB / link.travelA * (link.reverse ? -1 : 1)
  const ratio = driver == 0 ? forwardRatio : 1 / forwardRatio
  if(!Number.isFinite(ratio)) return 0
  const targetAccumulated = driverAccumulated * ratio
  const drivenRaw = jointMotionValue(tree, joints[driven], motions[driven])
  const drivenAccumulated = state[driven].accumulated + motionDelta(
    drivenRaw,
    state[driven].raw,
    isRotationalMotion(motions[driven]),
  )
  const error = targetAccumulated - drivenAccumulated
  applyJointMotionDelta(tree, joints[driven], motions[driven], error, fixed)
  return Math.abs(error)
}

function updateMotionLinkState(tree, link, joints) {
  const motions = [link.motionA, link.motionB]
  const state = ensureMotionLinkState(tree, link, joints)
  state.forEach((entry, index) => {
    const raw = jointMotionValue(tree, joints[index], motions[index])
    entry.accumulated += motionDelta(raw, entry.raw, isRotationalMotion(motions[index]))
    entry.raw = raw
  })
}

function applyJointMotionDelta(tree, joint, motion, delta, fixed) {
  if(Math.abs(delta) < EPSILON) return
  const componentA = tree.findChild(joint.componentA)
  const componentB = tree.findChild(joint.componentB)
  if(!componentA || !componentB) return
  const groupA = connectedRigidGroup(tree, componentA, tree.assemblyJoints || [])
  const groupB = connectedRigidGroup(tree, componentB, tree.assemblyJoints || [])
  const moveB = !groupB.some(component => fixed.has(component.id))
  const components = moveB ? groupB : groupA
  if(components.some(component => fixed.has(component.id))) return
  const attachmentA = worldTransform(componentA).multiply(joint.frameA)
  const pivot = framePosition(attachmentA)
  const axis = new THREE.Vector3().setFromMatrixColumn(attachmentA, motionAxis(motion)).normalize()
  const signedDelta = moveB ? delta : -delta
  if(isRotationalMotion(motion)) {
    const rotation = new THREE.Quaternion().setFromAxisAngle(axis, signedDelta)
    rotateWorldGroup(components, pivot, rotation)
  } else {
    translateWorldGroup(components, axis.multiplyScalar(signedDelta))
  }
}

function solveKinematicPath(tree, movedComponent, target, joints, fixed, iterations) {
  const adjacency = new Map()
  joints.filter(joint => joint.type != 'fix').forEach(joint => {
    const add = (from, to) => {
      if(!adjacency.has(from)) adjacency.set(from, [])
      adjacency.get(from).push({ joint, componentId: to })
    }
    add(joint.componentA, joint.componentB)
    add(joint.componentB, joint.componentA)
  })

  const queue = [movedComponent.id]
  const previous = new Map([[movedComponent.id, null]])
  let anchorId
  while(queue.length && !anchorId) {
    const componentId = queue.shift()
    if(fixed.has(componentId)) {
      anchorId = componentId
      break
    }
    for(const edge of adjacency.get(componentId) || []) {
      if(previous.has(edge.componentId)) continue
      previous.set(edge.componentId, { componentId, joint: edge.joint })
      queue.push(edge.componentId)
    }
  }
  if(!anchorId) return false

  const components = []
  const pathJoints = []
  let componentId = anchorId
  while(componentId != movedComponent.id) {
    const step = previous.get(componentId)
    components.unshift(tree.findChild(step.componentId))
    pathJoints.unshift(step.joint)
    componentId = step.componentId
  }
  if(pathJoints.some(joint => joint.type != 'axis')) return false

  for(let iteration = 0; iteration < iterations; iteration++) {
    for(let index = 0; index < pathJoints.length; index++) {
      const joint = pathJoints[index]
      const moving = components[index]
      const anchor = tree.findChild(
        joint.componentA == moving.id ? joint.componentB : joint.componentA
      )
      const anchorFrame = joint.componentA == anchor.id ? joint.frameA : joint.frameB
      const attachment = worldTransform(anchor).multiply(anchorFrame)
      const pivot = framePosition(attachment)
      const movingGroup = components.slice(0, index + 1)
      let from = framePosition(worldTransform(movedComponent)).sub(pivot)
      const to = target.clone().sub(pivot)
      const axis = frameDirection(attachment)

      if(!joint.lockSlide) {
        const distance = to.dot(axis) - from.dot(axis)
        translateWorldGroup(movingGroup, axis.clone().multiplyScalar(distance))
        from = framePosition(worldTransform(movedComponent)).sub(pivot)
      }

      from.sub(axis.clone().multiplyScalar(from.dot(axis)))
      to.sub(axis.clone().multiplyScalar(to.dot(axis)))
      if(from.lengthSq() < EPSILON || to.lengthSq() < EPSILON) continue
      from.normalize()
      to.normalize()
      const angle = Math.atan2(axis.dot(from.clone().cross(to)), from.dot(to))
      const rotation = new THREE.Quaternion().setFromAxisAngle(axis, angle)

      rotateWorldGroup(movingGroup, pivot, rotation)
    }
    if(framePosition(worldTransform(movedComponent)).distanceTo(target) < SOLVER_TOLERANCE) break
  }
  return true
}

function rotateWorldGroup(components, pivot, rotation) {
  const transform = new THREE.Matrix4().makeTranslation(pivot)
    .multiply(new THREE.Matrix4().makeRotationFromQuaternion(rotation))
    .multiply(new THREE.Matrix4().makeTranslation(pivot.clone().negate()))
  transformWorldGroup(components, transform)
}

function transformWorldGroup(components, transform) {
  const worlds = new Map(components.map(component => [
    component,
    transform.clone().multiply(worldTransform(component)),
  ]))
  components.slice().sort((a, b) => componentDepth(a) - componentDepth(b))
    .forEach(component => setWorldTransform(component, worlds.get(component)))
}

function translateWorldGroup(components, translation) {
  const transform = new THREE.Matrix4().makeTranslation(translation)
  transformWorldGroup(components, transform)
}

function connectedRigidGroup(tree, component, joints) {
  const groupJoints = joints.filter(joint => joint.type == 'group')
  const ids = new Set([component.id])
  const queue = [component.id]
  while(queue.length) {
    const id = queue.shift()
    groupJoints.forEach(joint => {
      const other = joint.componentA == id ? joint.componentB :
        joint.componentB == id ? joint.componentA : null
      if(!other || ids.has(other)) return
      ids.add(other)
      queue.push(other)
    })
  }
  return [...ids].map(id => tree.findChild(id)).filter(Boolean)
}

function projectRigidGroup(tree, anchor, joints) {
  const groupJoints = joints.filter(joint => joint.type == 'group')
  const worlds = new Map([[anchor, worldTransform(anchor)]])
  const queue = [anchor]
  while(queue.length) {
    const component = queue.shift()
    for(const joint of groupJoints) {
      const isA = joint.componentA == component.id
      const otherId = isA ? joint.componentB :
        joint.componentB == component.id ? joint.componentA : null
      const other = otherId && tree.findChild(otherId)
      if(!other || worlds.has(other)) continue
      const fromFrame = isA ? joint.frameA : joint.frameB
      const toFrame = isA ? joint.frameB : joint.frameA
      worlds.set(other, worlds.get(component).clone().multiply(fromFrame).multiply(toFrame.clone().invert()))
      queue.push(other)
    }
  }
  [...worlds.keys()].sort((a, b) => componentDepth(a) - componentDepth(b))
    .forEach(component => setWorldTransform(component, worlds.get(component)))
}

function componentDepth(component) {
  let depth = 0
  while(component.parent) {
    depth++
    component = component.parent
  }
  return depth
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

  if(joint.type == 'group') {
    const desiredA = attachmentB.clone().multiply(joint.frameA.clone().invert())
    const desiredB = attachmentA.clone().multiply(joint.frameB.clone().invert())
    if(mobilityA) setWorldTransform(componentA,
      mobilityB ? interpolateWorld(worldA, desiredA, weightA * RELAXATION) : desiredA
    )
    if(mobilityB) setWorldTransform(componentB,
      mobilityA ? interpolateWorld(worldB, desiredB, weightB * RELAXATION) : desiredB
    )
    return
  }

  if(joint.type == 'ball' || joint.type == 'axis') {
    let targetA = framePosition(attachmentB)
    let targetB = framePosition(attachmentA)
    if(joint.type == 'axis') {
      const radialError = framePosition(attachmentA).sub(framePosition(attachmentB))
      if(!joint.lockSlide) {
        const axis = frameDirection(attachmentA)
        radialError.sub(axis.multiplyScalar(radialError.dot(axis)))
      }
      targetA = framePosition(attachmentA).sub(radialError)
      targetB = framePosition(attachmentB).add(radialError)
    }
    if(mobilityA) {
      worldA = rotateAttachmentToward(worldA, framePosition(attachmentA), targetA, weightA * RELAXATION)
      setWorldTransform(componentA, worldA)
    }
    if(mobilityB) {
      worldB = rotateAttachmentToward(worldB, framePosition(attachmentB), targetB, weightB * RELAXATION)
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
  if(joint.type == 'axis' && !joint.lockSlide) {
    const axis = frameDirection(attachmentA)
    error.sub(axis.multiplyScalar(error.dot(axis)))
  } else if(joint.type == 'coplanar') {
    const normal = frameDirection(attachmentA)
    error = normal.multiplyScalar(error.dot(normal))
  }

  if(mobilityA) setWorldTransform(componentA, translateWorld(worldTransform(componentA), error.clone().multiplyScalar(-weightA * RELAXATION)))
  if(mobilityB) setWorldTransform(componentB, translateWorld(worldTransform(componentB), error.clone().multiplyScalar(weightB * RELAXATION)))
}

function interpolateWorld(from, to, weight) {
  const fromPosition = new THREE.Vector3()
  const fromRotation = new THREE.Quaternion()
  const fromScale = new THREE.Vector3()
  from.decompose(fromPosition, fromRotation, fromScale)
  const toPosition = new THREE.Vector3()
  const toRotation = new THREE.Quaternion()
  const toScale = new THREE.Vector3()
  to.decompose(toPosition, toRotation, toScale)
  return new THREE.Matrix4().compose(
    fromPosition.lerp(toPosition, weight),
    fromRotation.slerp(toRotation, weight),
    fromScale.lerp(toScale, weight),
  )
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

function assemblyError(tree, joints) {
  return joints.reduce((largest, joint) => {
    if(joint.type == 'fix') return largest
    const componentA = tree.findChild(joint.componentA)
    const componentB = tree.findChild(joint.componentB)
    if(!componentA || !componentB) return largest
    const attachmentA = worldTransform(componentA).multiply(joint.frameA)
    const attachmentB = worldTransform(componentB).multiply(joint.frameB)
    const directionA = frameDirection(attachmentA)
    const directionB = frameDirection(attachmentB)
    if(directionA.dot(directionB) < 0) directionB.negate()
    let positional = framePosition(attachmentA).sub(framePosition(attachmentB))
    if(joint.type == 'axis' && !joint.lockSlide) {
      positional.sub(directionA.clone().multiplyScalar(positional.dot(directionA)))
    } else if(joint.type == 'coplanar') {
      positional = directionA.clone().multiplyScalar(positional.dot(directionA))
    }
    const angular = joint.type == 'ball' ? 0 : joint.type == 'group' ?
      new THREE.Quaternion().setFromRotationMatrix(attachmentA)
        .angleTo(new THREE.Quaternion().setFromRotationMatrix(attachmentB)) :
      directionA.cross(directionB).length()
    return Math.max(largest, positional.length(), angular)
  }, 0)
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
