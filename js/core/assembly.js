import * as THREE from 'three'
import { solveLinear } from './linear-system.js'


const IDENTITY = () => new THREE.Matrix4()
const EPSILON = 1e-8

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
    result = alignDirection(result, framePosition(attachmentB), directionB, directionA)
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
  return result.premultiply(new THREE.Matrix4().makeTranslation(delta))
}

// Joint coordinates are measured from the joint's zero: the relative pose of
// the attachments where the joint feature aligned component B, before any rest
// offset. Translation and rotation are expressed in attachment A's frame.
function jointOffset(joint, attachmentA, attachmentB) {
  const relative = attachmentA.clone().invert().multiply(attachmentB)
  const zero = joint.zero
  return {
    translation: framePosition(relative).sub(framePosition(zero)),
    rotation: new THREE.Quaternion().setFromRotationMatrix(relative)
      .multiply(new THREE.Quaternion().setFromRotationMatrix(zero).invert()),
    direction: frameDirection(relative),
    zeroDirection: frameDirection(zero),
  }
}

// World transform that moves component B from the joint's zero to the given
// offset. Rotations turn about B's attachment so slide values are unaffected.
export function jointRestTransform(attachmentA, zero, translation, rotation) {
  const pivot = framePosition(zero)
  return attachmentA.clone()
    .multiply(new THREE.Matrix4().makeTranslation(pivot.clone().add(translation)))
    .multiply(new THREE.Matrix4().makeRotationFromQuaternion(rotation))
    .multiply(new THREE.Matrix4().makeTranslation(pivot.negate()))
    .multiply(attachmentA.clone().invert())
}

// Rotation about Z, ignoring the swing a solver residual may add.
function twistAngle(rotation) {
  return 2 * Math.atan2(rotation.z, rotation.w)
}

// Ranges of a full turn or more cannot be enforced without tracking turns, so
// angles wrap around the middle of the range and stay inside it.
function wrapAngle(angle, center) {
  return center + THREE.MathUtils.euclideanModulo(angle - center + Math.PI, Math.PI * 2) - Math.PI
}

function limitExcess(value, [min, max]) {
  return value < min ? value - min : value > max ? value - max : 0
}

// World space moves of component B that bring the joint back into its limits.
// Applying them negated to component A has the same relative effect.
export function limitCorrections(joint, attachmentA, attachmentB) {
  const corrections = []
  let error = 0
  const limits = joint.limits
  if(!limits) return { corrections, error }
  const offset = jointOffset(joint, attachmentA, attachmentB)
  const pivot = framePosition(attachmentB)
  const axisA = index => new THREE.Vector3().setFromMatrixColumn(attachmentA, index).normalize()

  if(limits.rotateZ) {
    const [min, max] = limits.rotateZ
    const excess = limitExcess(wrapAngle(twistAngle(offset.rotation), (min + max) / 2), limits.rotateZ)
    if(excess) corrections.push({ pivot, axis: axisA(2), angle: -excess })
    error = Math.max(error, Math.abs(excess))
  }
  ;['slideX', 'slideY', 'slideZ'].forEach((motion, index) => {
    if(!limits[motion]) return
    const excess = limitExcess(offset.translation.getComponent(index), limits[motion])
    if(excess) corrections.push({ translation: axisA(index).multiplyScalar(-excess) })
    error = Math.max(error, Math.abs(excess))
  })
  if(limits.tilt) {
    const excess = limitExcess(offset.direction.angleTo(offset.zeroDirection), limits.tilt)
    const axis = offset.direction.clone().cross(offset.zeroDirection)
    if(excess && axis.lengthSq() > EPSILON) {
      corrections.push({ pivot, axis: axis.normalize().transformDirection(attachmentA), angle: excess })
    }
    error = Math.max(error, Math.abs(excess))
  }
  return { corrections, error }
}

// Represent a drag by joint coordinates. Forward kinematics preserves the tree
// joints exactly; the remaining edges constrain closed loops and extra anchors.
export function solveAssembly(tree, movedComponent, desiredWorld, iterations = 64, targetLocal, pointerTravel = Infinity) {
  if(!movedComponent?.parent) return
  const model = dragMechanism(tree, movedComponent)
  if(!model.edges.length) {
    setWorldTransform(movedComponent, model.fixed.get(movedComponent.id) || desiredWorld)
    return
  }

  const variables = independentCoordinates(model)
  const origin = targetLocal || new THREE.Vector3()
  const goal = origin.clone().applyMatrix4(desiredWorld)
  const start = worldTransform(movedComponent)
  const grip = origin.clone().applyMatrix4(start)
  // Pointer drags specify one point. A programmatic rotation also specifies
  // orientation, including when the component origin lies on the bearing.
  const turn = !targetLocal && rotationError(start, desiredWorld).lengthSq() > EPSILON ** 2
  const orientationScale = Math.max(1, ...model.edges.map(e => e.arm))
  const evaluate = (values, previous) => {
    const worlds = model.place(values, model.targetOrder)
    const moved = worlds.get(movedComponent.id)
    const picked = origin.clone().applyMatrix4(moved)
    const target = picked.clone().sub(goal).toArray()
    const orientation = targetLocal ? null : rotationError(desiredWorld, moved, previous?.orientation)
    const attitude = orientation ? orientation.toArray().map(value => value * orientationScale) : []
    if(turn) target.push(...attitude)
    const closure = []
    for(const edge of model.closures) {
      const expected = worlds.get(edge.a).clone().multiply(edge.transform(values))
      const actual = worlds.get(edge.b)
      closure.push(...framePosition(expected).sub(framePosition(actual)).toArray(),
        ...rotationError(expected, actual).multiplyScalar(orientationScale).toArray())
    }
    const inequalities = []
    for(const edge of model.tiltLimits) {
      const tilt = frameDirection(edge.relative(values)).angleTo(edge.zeroDirection)
      const [min, max] = edge.joint.limits.tilt
      if(min > 0) inequalities.push((min - tilt) * orientationScale)
      if(max < Math.PI) inequalities.push((tilt - max) * orientationScale)
    }
    return { target, closure, inequalities, picked, orientation, preference: turn ? [] : attitude }
  }
  const values = fitDrag(variables, evaluate, grip, pointerTravel, iterations)
  if(!values) return
  const worlds = model.place(values)
  // A floating mechanism can use its remaining rigid-body freedom to follow
  // the grab. Physically separate followers of a motion link keep their bases.
  if(model.floating.has(movedComponent.id)) {
    const moved = worlds.get(movedComponent.id)
    const carry = turn ? desiredWorld.clone().multiply(moved.clone().invert()) :
      new THREE.Matrix4().makeTranslation(goal.clone().sub(origin.clone().applyMatrix4(moved)))
    for(const id of model.floating) worlds.get(id).premultiply(carry)
  }
  const components = [...worlds.keys()].map(id => tree.findChild(id))
  components.sort((a, b) => componentDepth(a) - componentDepth(b))
    .forEach(c => setWorldTransform(c, worlds.get(c.id)))
  for(const { link, a, b, pair } of model.links) {
    for(const [i, coord] of [a, b].entries()) {
      const state = link.state[i]
      state.accumulated += coord.rate * values[coord.index]
      state.raw = jointMotionValue(tree, pair[i].joint, coord.motion)
      if(pair[i].joint.type == 'ball') state.angles = pair[i].angles(values).toArray().slice(0, 3)
    }
  }
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

function ensureMotionLinkState(tree, link, joints) {
  if(link.state?.length == 2) return link.state
  link.state = joints.map((joint, index) => {
    const raw = jointMotionValue(tree, joint, index ? link.motionB : link.motionA)
    return { raw, accumulated: 0 }
  })
  return link.state
}

function rotationError(from, to, previous) {
  const q = new THREE.Quaternion().setFromRotationMatrix(from).invert()
    .multiply(new THREE.Quaternion().setFromRotationMatrix(to))
  const v = new THREE.Vector3(q.x, q.y, q.z)
  const length = v.length()
  if(length < EPSILON) return v.multiplyScalar(q.w < 0 ? -2 : 2)
  // Both quaternion signs describe the same half turn. Differentiate on the
  // same side of that cut for the two nearby samples of the Jacobian.
  const sign = previous && Math.abs(q.w) < 1e-3 ? Math.sign(v.dot(previous)) || 1 : Math.sign(q.w) || 1
  return v.multiplyScalar(sign * 2 * Math.atan2(length, sign * q.w) / length)
}

function connect(graph, a, b) {
  if(!graph.has(a)) graph.set(a, [])
  if(!graph.has(b)) graph.set(b, [])
  graph.get(a).push(b)
  graph.get(b).push(a)
}

function reachable(graph, start) {
  const seen = new Set([start])
  for(const id of seen) {
    for(const neighbor of graph.get(id) || []) seen.add(neighbor)
  }
  return seen
}

// Each edge describes B in A's attachment frame, even if the spanning tree
// later traverses it backwards. Linked travel and limits use that convention.
function kinematicEdge(joint, tree, previousAngles) {
  const { componentA: a, componentB: b, frameA, frameB } = joint
  const inverseB = frameB.clone().invert()
  const start = joint.type == 'group' ? IDENTITY() :
    worldTransform(tree.findChild(a)).multiply(frameA).invert()
      .multiply(worldTransform(tree.findChild(b))).multiply(frameB)
  const position = framePosition(start)
  const rotation = new THREE.Euler().setFromRotationMatrix(start)
  if(previousAngles) {
    // At an Euler pole the matrix alone cannot recover the linked X and Z
    // coordinates. Retain the solved chart while the attachment is still there.
    const previous = new THREE.Euler(...previousAngles)
    if(rotationError(new THREE.Matrix4().makeRotationFromEuler(previous), start).lengthSq() < 1e-16) {
      rotation.copy(previous)
    }
  }
  const coords = new Map()
  for(const motion of Object.keys(jointMotionOptions(joint))) {
    coords.set(motion, { motion, neighbors: [], low: -Infinity, high: Infinity })
  }
  const arm = Math.max(framePosition(frameA).length(), framePosition(frameB).length())
  // Components saved after an older drag can carry residual bearing error.
  // Keep only the position and rotation coordinates this joint allows.
  for(const [index, name] of ['slideX', 'slideY', 'slideZ'].entries()) {
    if(!coords.has(name)) position.setComponent(index, 0)
  }
  if(joint.type == 'axis' || joint.type == 'coplanar') {
    rotation.x = frameDirection(joint.zero || start).z < 0 ? Math.PI : 0
    rotation.y = 0
  }
  const aligned = new THREE.Matrix4().makeRotationFromEuler(rotation).setPosition(position)
  const zero = joint.zero || aligned
  for(const [motion, coord] of coords) {
    const limits = joint.limits?.[motion]
    if(!limits) continue
    const angle = isRotationalMotion(motion)
    if(angle && limits[1] - limits[0] >= 2 * Math.PI) continue
    const sign = angle ? Math.sign(frameDirection(aligned).z) : 1
    const offset = jointOffset({ zero }, IDENTITY(), aligned)
    const current = angle ? wrapAngle(twistAngle(offset.rotation), (limits[0] + limits[1]) / 2) :
      offset.translation.getComponent(motionAxis(motion))
    const bounds = limits.map(value => (value - current) * sign)
    coord.low = Math.min(...bounds)
    coord.high = Math.max(...bounds)
  }
  const angles = values => {
    const r = rotation.clone()
    for(const [motion, c] of coords) {
      if(isRotationalMotion(motion)) r[motion.at(-1).toLowerCase()] += values[c.index] * c.rate
    }
    return r
  }
  const relative = values => {
    const p = position.clone()
    for(const [motion, c] of coords) {
      if(!isRotationalMotion(motion)) p[motion.at(-1).toLowerCase()] += values[c.index] * c.rate
    }
    if(joint.type == 'ball' && ![...coords.values()].some(c => c.neighbors.length)) {
      // Local incremental rotations have three independent axes even when
      // the absolute XYZ Euler representation is at a pole.
      const delta = [...coords.values()].map(c => values[c.index] * c.rate)
      return aligned.clone().multiply(new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(...delta)))
    }
    return new THREE.Matrix4().makeRotationFromEuler(angles(values)).setPosition(p)
  }
  const rigid = frameA.clone().multiply(inverseB)
  const transform = coords.size ? values => frameA.clone().multiply(relative(values)).multiply(inverseB) : () => rigid
  return { joint, a, b, coords, relative, transform, arm, angles, zeroDirection: frameDirection(zero) }
}

function dragMechanism(tree, moved) {
  const joints = tree.assemblyJoints || []
  const physical = new Map(), connected = new Map()
  const byId = new Map(joints.map(j => [j.id, j]))
  for(const j of joints.filter(j => j.type != 'fix')) {
    connect(physical, j.componentA, j.componentB)
    connect(connected, j.componentA, j.componentB)
  }
  const previousAngles = new Map()
  for(const link of tree.motionLinks || []) {
    connect(connected, byId.get(link.jointA).componentA, byId.get(link.jointB).componentA)
    if(link.state?.[0].angles) previousAngles.set(link.jointA, link.state[0].angles)
    if(link.state?.[1].angles) previousAngles.set(link.jointB, link.state[1].angles)
  }
  const active = reachable(connected, moved.id)
  const fixed = new Map(joints.filter(j => j.type == 'fix' && active.has(j.componentA))
    .map(j => [j.componentA, j.fixedWorld]))
  const edges = joints.filter(j => j.type != 'fix' && active.has(j.componentA))
    .map(j => kinematicEdge(j, tree, previousAngles.get(j.id)))
  const byJoint = new Map(edges.map(e => [e.joint.id, e]))
  const links = (tree.motionLinks || []).filter(link => byJoint.has(link.jointA)).map(link => {
    const pair = [byJoint.get(link.jointA), byJoint.get(link.jointB)]
    const joints = pair.map(e => e.joint)
    ensureMotionLinkState(tree, link, joints)
    return { link, pair, a: pair[0].coords.get(link.motionA), b: pair[1].coords.get(link.motionB) }
  })

  // Include rigid edges first so grouped parts share a transform even in a
  // closed linkage. Multiple fixed parts act as roots of the same forest.
  const parent = new Map()
  const rootOf = id => {
    while(parent.has(id)) id = parent.get(id)
    return id
  }
  const ground = Symbol('ground')
  for(const id of fixed.keys()) parent.set(id, ground)
  const branches = new Map([...active].map(id => [id, []]))
  const closures = []
  for(const e of [...edges].sort((a, b) => Number(b.joint.type == 'group') - Number(a.joint.type == 'group'))) {
    const ra = rootOf(e.a), rb = rootOf(e.b)
    if(ra == rb) closures.push(e)
    else {
      parent.set(ra, rb)
      branches.get(e.a).push(e)
      branches.get(e.b).push(e)
    }
  }
  const roots = new Map(), order = [], seen = new Set()
  for(const id of [...fixed.keys(), ...edges.map(e => e.a).filter(id => id != moved.id), ...active]) {
    if(seen.has(id)) continue
    roots.set(id, fixed.get(id) || worldTransform(tree.findChild(id)))
    const queue = [id]
    seen.add(id)
    for(const from of queue) {
      for(const edge of branches.get(from)) {
        const reverse = from == edge.b
        const to = reverse ? edge.a : edge.b
        if(seen.has(to)) continue
        seen.add(to)
        queue.push(to)
        order.push({ edge, from, to, reverse })
      }
    }
  }
  // Only the ancestor paths of the grip and the loop closures affect the fit.
  // Followers on other branches are placed once when committing the solution.
  const needed = new Set([moved.id, ...closures.flatMap(e => [e.a, e.b])])
  for(let i = order.length - 1; i >= 0; i--) {
    if(needed.has(order[i].to)) needed.add(order[i].from)
  }
  const targetOrder = order.filter(step => needed.has(step.to))
  const tiltLimits = edges.filter(e => e.joint.limits?.tilt)
  for(const e of [...targetOrder.map(s => s.edge), ...closures, ...tiltLimits]) {
    for(const coord of e.coords.values()) coord.affectsTarget = true
  }
  const place = (values, steps = order) => {
    const worlds = new Map(roots)
    for(const { edge, from, to, reverse } of steps) {
      const relative = edge.transform(values)
      worlds.set(to, worlds.get(from).clone().multiply(reverse ? relative.clone().invert() : relative))
    }
    return worlds
  }
  const floating = reachable(physical, moved.id)
  if([...floating].some(id => fixed.has(id))) floating.clear()
  return { fixed, floating, edges, links, closures, place, targetOrder, tiltLimits }
}

// A connected set of scalar motion links contributes one independent
// displacement. Intersect all of its members' travel ranges in that coordinate.
function independentCoordinates({ edges, links }) {
  for(const { link, a, b } of links) {
    const ratio = link.travelB / link.travelA * (link.reverse ? -1 : 1)
    a.neighbors.push([b, ratio])
    b.neighbors.push([a, 1 / ratio])
  }
  const variables = []
  for(const edge of edges) for(const first of edge.coords.values()) {
    if(first.index !== undefined) continue
    const index = variables.length
    let low = -Infinity, high = Infinity, maxRotation = 0
    const rates = new Map([[first, 1]])
    for(const [coord, rate] of rates) {
      coord.index = index
      coord.rate = rate
      if(isRotationalMotion(coord.motion)) maxRotation = Math.max(maxRotation, Math.abs(rate))
      const lo = coord.low / rate, hi = coord.high / rate
      low = Math.max(low, Math.min(lo, hi))
      high = Math.min(high, Math.max(lo, hi))
      for(const [other, ratio] of coord.neighbors) {
        if(!rates.has(other)) rates.set(other, rate * ratio)
        // An inconsistent ratio cycle permits only zero incremental travel.
        else if(Math.abs(rates.get(other) / (rate * ratio) - 1) > EPSILON) low = high = 0
      }
    }
    variables.push({ low, high, maxRotation, active: [...rates.keys()].some(c => c.affectsTarget) })
  }
  return variables
}

const dot = (a, b) => a.reduce((sum, value, i) => sum + value * b[i], 0)
const normSq = a => dot(a, a)

function linearize(values, evaluate, state, variables, closureOnly) {
  const row = () => Array(values.length).fill(0)
  const hard = state.closure.map(row), inequalities = state.inequalities.map(row)
  const soft = closureOnly ? [] : state.target.map(row)
  const preference = closureOnly ? [] : state.preference.map(row)
  for(let i = 0; i < values.length; i++) {
    if(!variables[i].active || variables[i].low == variables[i].high) continue
    const h = 1e-5
    const sample = values.slice()
    sample[i] += h
    const plus = evaluate(sample, state)
    sample[i] -= 2 * h
    const minus = evaluate(sample, state)
    hard.forEach((row, k) => { row[i] = (plus.closure[k] - minus.closure[k]) / (2 * h) })
    inequalities.forEach((row, k) => { row[i] = (plus.inequalities[k] - minus.inequalities[k]) / (2 * h) })
    soft.forEach((row, k) => { row[i] = (plus.target[k] - minus.target[k]) / (2 * h) })
    preference.forEach((row, k) => { row[i] = (plus.preference[k] - minus.preference[k]) / (2 * h) })
  }
  return { hard, soft, preference, inequalities }
}

// Satisfy the linearized closure constraints first, then fit the cursor in
// their null space. Redundant constraints do not create additional DOFs.
function constrainedStep({ hard, soft, preference, inequalities }, state, values, variables) {
  const basis = []
  const correction = values.map(() => 0)
  const project = row => {
    const out = row.slice()
    for(const normal of basis) {
      const amount = dot(normal, out)
      for(let i = 0; i < out.length; i++) out[i] -= amount * normal[i]
    }
    return out
  }
  const constrain = (row, rhs) => {
    const free = project(row)
    const magnitude = Math.sqrt(normSq(free))
    if(magnitude <= 1e-8 * Math.max(1, Math.sqrt(normSq(row)))) return
    const normal = free.map(x => x / magnitude)
    const distance = (rhs - dot(row, correction)) / magnitude
    for(let i = 0; i < correction.length; i++) correction[i] += distance * normal[i]
    basis.push(normal)
  }
  hard.forEach((row, i) => constrain(row, -state.closure[i]))
  const fit = (rows, residual, start) => {
    const free = rows.map(project)
    // Damped least squares in task space: at most six rows for a pose.
    const gram = free.map(a => free.map(b => dot(a, b)))
    const damping = 1e-9 * Math.max(1, ...gram.map((row, i) => row[i]))
    gram.forEach((row, i) => { row[i] += damping })
    const weights = solveLinear(gram, rows.map((row, i) => -residual[i] - dot(row, start)))
    return start.map((x, k) => x + free.reduce((sum, row, i) => sum + row[k] * weights[i], 0))
  }
  const stops = new Set()
  const activeInequalities = new Set()
  while(true) {
    let step = fit(soft, state.target, correction)
    if(preference.length) {
      // When translating a pose, keep its attitude with any freedom left
      // after fitting the position. This selects a continuous elbow branch.
      const rank = basis.length
      for(const row of soft) {
        const free = project(row), length = Math.sqrt(normSq(free))
        if(length > EPSILON) basis.push(free.map(x => x / length))
      }
      step = fit(preference, state.preference, step)
      basis.length = rank
    }
    const stop = variables.findIndex(({ low, high }, i) => !stops.has(i) &&
      Math.abs(limitExcess(values[i] + step[i], [low, high])) > EPSILON)
    if(stop < 0) {
      const k = inequalities.findIndex((row, i) => !activeInequalities.has(i) &&
        state.inequalities[i] + dot(row, step) > 1e-10)
      if(k < 0) return step
      activeInequalities.add(k)
      constrain(inequalities[k], -state.inequalities[k])
      continue
    }
    stops.add(stop)
    const { low, high } = variables[stop]
    const bound = THREE.MathUtils.clamp(values[stop] + step[stop], low, high)
    constrain(values.map((_, i) => Number(i == stop)), bound - values[stop])
  }
}

function advance(values, delta, factor, variables) {
  return values.map((x, i) => THREE.MathUtils.clamp(x + factor * delta[i], variables[i].low, variables[i].high))
}

// Retract trial steps onto closed loops (and ball-joint tilt stops) before
// publishing a pose. An unreachable target must never pull a linkage apart.
function closeLoops(values, state, evaluate, variables) {
  for(let i = 0; i < 10 && constraintError(state) > 1e-18; i++) {
    const jacobian = linearize(values, evaluate, state, variables, true)
    const correction = constrainedStep(jacobian, state, values, variables)
    const trial = advance(values, correction, 1, variables)
    const next = evaluate(trial)
    if(constraintError(next) >= constraintError(state)) break
    values = trial
    state = next
  }
  return { values, state }
}

function constraintError(state) {
  return state.inequalities.reduce((sum, value) => sum + Math.max(0, value) ** 2, normSq(state.closure))
}

// At a straight elbow an inward drag has zero first derivative. Try a small
// bend in the null space of the grip Jacobian before declaring it unreachable.
// A single hinge dragged diametrically across its pivot needs a direct nudge.
function escapeSingularity(jacobian, values, variables, evaluate, accept) {
  const rows = [...jacobian.hard, ...jacobian.soft]
  for(let i = 0; i < variables.length; i++) {
    const v = variables[i]
    if(!v.active || !v.maxRotation || v.low == v.high) continue
    const axis = values.map((_, k) => Number(k == i))
    const nudge = 0.001 / v.maxRotation
    const bend = constrainedStep(
      { hard: rows, soft: [axis], inequalities: [], preference: [] },
      { closure: rows.map(() => 0), target: [-nudge] }, values, variables,
    )
    for(const delta of [bend, axis.map(x => x * nudge)]) {
      if(normSq(delta) < EPSILON ** 2) continue
      for(const sign of [1, -1]) {
        const trial = advance(values, delta, sign, variables)
        const next = closeLoops(trial, evaluate(trial), evaluate, variables)
        if(accept(next.state)) return next
      }
    }
  }
}

function fitDrag(variables, evaluate, grip, pointerTravel, iterations) {
  if(variables.some(v => v.low > v.high)) return null
  const initial = variables.map(v => THREE.MathUtils.clamp(0, v.low, v.high))
  let { values, state } = closeLoops(initial, evaluate(initial), evaluate, variables)
  for(let i = 0; i < iterations && variables.length; i++) {
    const error = normSq(state.target)
    if(error < 1e-18 || pointerTravel == 0) break
    const jacobian = linearize(values, evaluate, state, variables, false)
    const delta = constrainedStep(jacobian, state, values, variables)
    const maxAngle = Math.max(0, ...delta.map((x, k) => Math.abs(x) * variables[k].maxRotation))
    let factor = maxAngle > 0.5 ? 0.5 / maxAngle : 1
    const better = next => constraintError(next) < 1e-16 && normSq(next.target) < error &&
      next.picked.distanceTo(grip) <= pointerTravel + 1e-10
    const length = Math.sqrt(normSq(delta))
    let accepted
    while(factor * length > 1e-10) {
      const trial = advance(values, delta, factor, variables)
      const next = closeLoops(trial, evaluate(trial), evaluate, variables)
      if(better(next.state)) {
        accepted = next
        break
      }
      factor *= 0.5
    }
    if(!accepted && i == 0 && length < EPSILON) {
      accepted = escapeSingularity(jacobian, values, variables, evaluate, better)
    }
    if(!accepted) break
    values = accepted.values
    state = accepted.state
  }
  // Incompatible constraints have no admissible pose. Keep the last placement.
  return constraintError(state) < 1e-16 ? values : null
}

function componentDepth(component) {
  let depth = 0
  while(component.parent) {
    depth++
    component = component.parent
  }
  return depth
}

function alignDirection(world, pivot, from, to) {
  if(from.dot(to) < 0) to = to.clone().negate()
  const rotation = new THREE.Quaternion().setFromUnitVectors(from, to)
  if(Math.abs(rotation.w - 1) < EPSILON) return world
  return new THREE.Matrix4().makeTranslation(pivot)
    .multiply(new THREE.Matrix4().makeRotationFromQuaternion(rotation))
    .multiply(new THREE.Matrix4().makeTranslation(pivot.clone().negate()))
    .multiply(world)
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
