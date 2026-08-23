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

  for(let i = 0; i < iterations; i++) {
    projectJoints()
    if(!pathSolved) setWorldTransform(movedComponent, desiredWorld)
    if(assemblyError(tree, joints) < SOLVER_TOLERANCE) return
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
