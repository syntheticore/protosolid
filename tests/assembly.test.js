import assert from 'node:assert/strict'
import test from 'node:test'
import * as THREE from 'three'

import {
  alignJointWorld,
  captureMotionLinkStates,
  jointMotionValue,
  restoreMotionLinkStates,
  solveAssembly,
  worldTransform,
} from '../js/core/assembly.js'
import { JointFeature, PatternFeature } from '../js/core/features.js'
import { Face } from '../js/core/geom3d.js'
import {
  Component,
  ComponentDefinition,
  createComponentInstance,
  syncComponentInstances,
} from '../js/core/component.js'


class TestComponent {
  constructor(id, parent, position = new THREE.Vector3()) {
    this.id = id
    this.parent = parent
    this.children = []
    this.designTransform = new THREE.Matrix4().makeTranslation(position)
    this.transform = null
    if(parent) parent.children.push(this)
  }

  typename() { return 'Component' }

  findChild(id) {
    if(this.id == id) return this
    for(const child of this.children) {
      const found = child.findChild(id)
      if(found) return found
    }
  }

  getChildren() {
    return [this, ...this.children.flatMap(child => child.getChildren())]
  }

  hasAncestor(parent) {
    return this == parent || !!this.parent?.hasAncestor(parent)
  }
}

const at = (x, y = 0, z = 0) => new THREE.Matrix4().makeTranslation(x, y, z)
const point = (component, frame = new THREE.Matrix4()) =>
  new THREE.Vector3().setFromMatrixPosition(worldTransform(component).multiply(frame))
const direction = (component, frame = new THREE.Matrix4()) =>
  new THREE.Vector3().setFromMatrixColumn(worldTransform(component).multiply(frame), 2).normalize()
const axisSeparation = (componentA, frameA, componentB, frameB) => {
  const axis = direction(componentA, frameA)
  const offset = point(componentA, frameA).sub(point(componentB, frameB))
  return offset.sub(axis.multiplyScalar(offset.dot(axis))).length()
}

function axialChain(length, lockSlide = false) {
  const root = new TestComponent('root')
  const links = Array.from({ length }, (_, index) =>
    new TestComponent(`link-${index}`, root, new THREE.Vector3(index, 0, 0))
  )
  root.assemblyJoints = [
    { type: 'fix', componentA: links[0].id, fixedWorld: new THREE.Matrix4() },
    ...links.slice(1).map((link, index) => ({
      type: 'axis',
      componentA: links[index].id,
      componentB: link.id,
      frameA: at(0.5),
      frameB: at(-0.5),
      lockSlide,
    })),
  ]
  return { root, links }
}

function linkedAxes(motionB = 'rotateZ', travelB = Math.PI, reverse = true) {
  const root = new TestComponent('root')
  const baseA = new TestComponent('base-a', root)
  const movingA = new TestComponent('moving-a', root)
  const baseB = new TestComponent('base-b', root)
  const movingB = new TestComponent('moving-b', root)
  root.assemblyJoints = [
    {
      id: 'joint-a',
      type: 'axis',
      componentA: baseA.id,
      componentB: movingA.id,
      frameA: new THREE.Matrix4(),
      frameB: new THREE.Matrix4(),
      lockSlide: true,
    },
    {
      id: 'joint-b',
      type: 'axis',
      componentA: baseB.id,
      componentB: movingB.id,
      frameA: new THREE.Matrix4(),
      frameB: new THREE.Matrix4(),
      lockSlide: motionB != 'slideZ',
    },
  ]
  root.motionLinks = [{
    id: 'motion-link',
    jointA: 'joint-a',
    motionA: 'rotateZ',
    travelA: Math.PI * 2,
    jointB: 'joint-b',
    motionB,
    travelB,
    reverse,
  }]
  return { root, movingA, movingB }
}

test('axis alignment removes radial offset while preserving axial slide', () => {
  const frameB = at(2, 0, 7)
  const aligned = alignJointWorld('axis', new THREE.Matrix4(), new THREE.Matrix4(), new THREE.Matrix4(), frameB)
  const attachment = new THREE.Vector3().setFromMatrixPosition(aligned.multiply(frameB))
  assert.ok(Math.hypot(attachment.x, attachment.y) < 1e-8)
  assert.ok(Math.abs(attachment.z - 7) < 1e-8)
})

test('locked axis alignment also removes axial offset', () => {
  const frameB = at(2, 0, 7)
  const aligned = alignJointWorld('axis', new THREE.Matrix4(), new THREE.Matrix4(), new THREE.Matrix4(), frameB, true)
  const attachment = new THREE.Vector3().setFromMatrixPosition(aligned.multiply(frameB))
  assert.ok(attachment.length() < 1e-8)
})

test('coplanar alignment removes normal offset while preserving planar position', () => {
  const frameB = at(3, 4, 5)
  const aligned = alignJointWorld('coplanar', new THREE.Matrix4(), new THREE.Matrix4(), new THREE.Matrix4(), frameB)
  const attachment = new THREE.Vector3().setFromMatrixPosition(aligned.multiply(frameB))
  assert.deepEqual(attachment.toArray().map(value => Math.round(value * 1e8) / 1e8), [3, 4, 0])
})

test('spherical faces expose their center as a point reference', () => {
  const previousWindow = globalThis.window
  globalThis.window = {
    oc: { oc: {
      BRepAdaptor_Surface_2: class {
        Sphere() {
          return { Location: () => ({ X: () => 3, Y: () => 4, Z: () => 5 }) }
        }
      },
    } },
  }
  try {
    const face = new Face({ compound: { componentId: 'component' }, id: 'solid' }, {})
    const center = new THREE.Vector3().setFromMatrixPosition(face.pointReference().getItem())
    assert.deepEqual(center.toArray(), [3, 4, 5])
  } finally {
    if(previousWindow === undefined) delete globalThis.window
    else globalThis.window = previousWindow
  }
})

test('fixed joints reject manipulation', () => {
  const root = new TestComponent('root')
  const fixed = new TestComponent('fixed', root)
  root.assemblyJoints = [{ type: 'fix', componentA: fixed.id, fixedWorld: new THREE.Matrix4() }]
  solveAssembly(root, fixed, at(20))
  assert.ok(point(fixed).length() < 1e-8)
})

test('motion links map reversed rotational travel in either joint coordinate', () => {
  const { root, movingA } = linkedAxes()
  solveAssembly(root, movingA, new THREE.Matrix4().makeRotationZ(Math.PI / 2))

  assert.ok(Math.abs(jointMotionValue(root, root.assemblyJoints[0], 'rotateZ') - Math.PI / 2) < 1e-8)
  assert.ok(Math.abs(jointMotionValue(root, root.assemblyJoints[1], 'rotateZ') + Math.PI / 4) < 1e-8)

  const reverse = linkedAxes()
  solveAssembly(reverse.root, reverse.movingB, new THREE.Matrix4().makeRotationZ(Math.PI / 4))
  assert.ok(Math.abs(jointMotionValue(reverse.root, reverse.root.assemblyJoints[0], 'rotateZ') + Math.PI / 2) < 1e-8)
})

test('motion links map rotational travel onto linear travel', () => {
  const { root, movingA } = linkedAxes('slideZ', 10, false)
  solveAssembly(root, movingA, new THREE.Matrix4().makeRotationZ(Math.PI / 2))

  assert.ok(Math.abs(jointMotionValue(root, root.assemblyJoints[1], 'slideZ') - 2.5) < 1e-8)
})

test('restored motion-link state makes repeated drag updates absolute', () => {
  const { root, movingA } = linkedAxes()
  const startPoses = new Map(root.getChildren().map(component => [
    component.id,
    component.transform && component.transform.clone(),
  ]))
  const startMotionLinkStates = captureMotionLinkStates(root)

  const angles = [Math.PI / 6, Math.PI / 3]
  angles.forEach(angle => {
    root.getChildren().forEach(component => {
      const pose = startPoses.get(component.id)
      component.transform = pose && pose.clone()
    })
    restoreMotionLinkStates(root, startMotionLinkStates)
    solveAssembly(root, movingA, new THREE.Matrix4().makeRotationZ(angle))
    assert.ok(Math.abs(jointMotionValue(root, root.assemblyJoints[1], 'rotateZ') + angle / 2) < 1e-8)
  })
})

test('joint picking only accepts strict descendants of the owning component', () => {
  const root = new TestComponent('root')
  const owner = new TestComponent('owner', root)
  const child = new TestComponent('child', owner)
  const grandchild = new TestComponent('grandchild', child)
  const sibling = new TestComponent('sibling', root)
  const feature = {
    componentId: owner.id,
    document: { top: () => root },
  }
  const accepts = item => JointFeature.prototype.acceptsInput.call(feature, item)

  assert.equal(accepts(owner), false)
  assert.equal(accepts(child), true)
  assert.equal(accepts(grandchild), true)
  assert.equal(accepts(sibling), false)
})

test('component instances share definition content but retain occurrence state', () => {
  const root = new Component(null, 'root')
  root.creator = new ComponentDefinition('Root', '#fff')
  const source = new Component(root, 'source')
  source.creator = new ComponentDefinition('Link', '#f00')
  source.compound = {
    revision: 1,
    cloneForComponent: componentId => ({
      componentId,
      revision: source.compound.revision,
      cloneForComponent: source.compound.cloneForComponent,
    }),
  }
  root.children.push(source)

  const instance = createComponentInstance(root, source, 'instance')
  instance.creator.hidden = true
  source.creator.title = 'Renamed Link'
  source.compound.revision = 2
  const sourceChild = new Component(source, 'source-child')
  sourceChild.creator = new ComponentDefinition('Pin', '#00f')
  sourceChild.compound = {
    cloneForComponent: componentId => ({ componentId, cloneForComponent: sourceChild.compound.cloneForComponent }),
  }
  source.children.push(sourceChild)
  syncComponentInstances(root)

  assert.equal(instance.instanceOf, source.id)
  assert.equal(instance.creator.title, 'Renamed Link')
  assert.equal(instance.creator.hidden, true)
  assert.equal(source.creator.hidden, false)
  assert.equal(instance.compound.componentId, instance.id)
  assert.equal(instance.compound.revision, 2)
  assert.ok(instance.children.some(child => child.sourceOccurrence == sourceChild.id))
  source.creator.itemsHidden[source.id + '/solid/0'] = true
  assert.equal(instance.isItemHidden(instance.id + '/solid/0'), true)
})

test('component pattern inputs create component instances instead of fused bodies', () => {
  const root = new Component(null, 'root')
  root.creator = new ComponentDefinition('Root', '#fff')
  const owner = new Component(root, 'owner')
  owner.creator = new ComponentDefinition('Assembly', '#0f0')
  root.children.push(owner)
  const source = new Component(owner, 'source')
  source.creator = new ComponentDefinition('Link', '#f00')
  source.compound = {
    cloneForComponent: componentId => ({ componentId, cloneForComponent: source.compound.cloneForComponent }),
  }
  owner.children.push(source)
  const document = {
    activeComponent: owner,
    selection: { items: [] },
    timeline: { features: [] },
  }
  const pattern = new PatternFeature(document)
  document.timeline.features.push(pattern)
  pattern.inputs = () => [{ featureId: null }]
  pattern.uCount = 2
  pattern.vCount = 1

  pattern.updateFeature(root, { inputs: [source] })

  const instance = owner.children.find(component => component.instanceOf == source.id)
  assert.ok(instance)
  assert.equal(instance.id, `${pattern.id}/instance/0/0`)
  assert.equal(instance.compound.componentId, instance.id)
})

test('dragging an axial chain rotates its links while keeping every joint axis coincident', () => {
  const { root, links } = axialChain(5)
  const target = new THREE.Vector3(3, 2, 0)
  solveAssembly(root, links.at(-1), at(...target.toArray()))

  root.assemblyJoints.slice(1).forEach((joint, index) => {
    assert.ok(point(links[index], joint.frameA).distanceTo(point(links[index + 1], joint.frameB)) < 1e-6)
    assert.ok(Math.abs(direction(links[index], joint.frameA).dot(direction(links[index + 1], joint.frameB))) > 1 - 1e-8)
  })
  assert.ok(point(links.at(-1)).distanceTo(target) < 1e-2)
  assert.ok(links.slice(1).some(link => Math.abs(new THREE.Vector3().setFromMatrixColumn(worldTransform(link), 0).y) > 0.1))
})

test('dragging a middle link carries the free side of the chain', () => {
  const { root, links } = axialChain(5, true)
  const moved = links[2]

  for(const target of [
    new THREE.Vector3(1.8, 0.4, 0),
    new THREE.Vector3(1.6, 0.8, 0),
    new THREE.Vector3(1.4, 1.1, 0),
  ]) {
    solveAssembly(root, moved, at(...target.toArray()))

    assert.ok(point(moved).distanceTo(target) < 1e-2)
    root.assemblyJoints.slice(1).forEach((joint, index) => {
      assert.ok(point(links[index], joint.frameA).distanceTo(point(links[index + 1], joint.frameB)) < 1e-6)
    })
  }
})

test('dragging away from a joint rotates a link whose origin is on the joint', () => {
  const root = new TestComponent('root')
  const anchor = new TestComponent('anchor', root)
  const link = new TestComponent('link', root)
  root.assemblyJoints = [
    { type: 'fix', componentA: anchor.id, fixedWorld: new THREE.Matrix4() },
    {
      type: 'axis',
      componentA: anchor.id,
      componentB: link.id,
      frameA: new THREE.Matrix4(),
      frameB: new THREE.Matrix4(),
      lockSlide: true,
    },
  ]
  const dragPoint = new THREE.Vector3(1, 0, 0)

  solveAssembly(root, link, at(-1, 1), 64, dragPoint)

  assert.ok(dragPoint.clone().applyMatrix4(worldTransform(link)).distanceTo(new THREE.Vector3(0, 1, 0)) < 1e-6)
  assert.ok(point(link).length() < 1e-8)
})

test('axial chain joints remain coincident when the drag target is unreachable', () => {
  for(const target of [new THREE.Vector3(8, 0, 0), new THREE.Vector3(0.75, 0.1, 0)]) {
    const { root, links } = axialChain(3)
    solveAssembly(root, links.at(-1), at(...target.toArray()))

    root.assemblyJoints.slice(1).forEach((joint, index) => {
      assert.ok(point(links[index], joint.frameA).distanceTo(point(links[index + 1], joint.frameB)) < 1e-6)
      assert.ok(Math.abs(direction(links[index], joint.frameA).dot(direction(links[index + 1], joint.frameB))) > 1 - 1e-8)
    })
    const anchor = point(links[0], at(0.5))
    const end = point(links.at(-1))
    assert.ok(end.distanceTo(anchor) >= 0.5 - 1e-6)
    assert.ok(end.distanceTo(anchor) <= 1.5 + 1e-6)
  }
})

test('axis joints slide freely unless axial movement is locked', () => {
  const sliding = axialChain(3)
  solveAssembly(sliding.root, sliding.links.at(-1), at(2, 0, 3))
  assert.ok(Math.abs(point(sliding.links.at(-1)).z - 3) < 1e-6)
  sliding.root.assemblyJoints.slice(1).forEach((joint, index) => {
    assert.ok(axisSeparation(sliding.links[index], joint.frameA, sliding.links[index + 1], joint.frameB) < 1e-6)
  })

  const locked = axialChain(3, true)
  solveAssembly(locked.root, locked.links.at(-1), at(2, 0, 3))
  assert.ok(Math.abs(point(locked.links.at(-1)).z) < 1e-6)
  locked.root.assemblyJoints.slice(1).forEach((joint, index) => {
    assert.ok(point(locked.links[index], joint.frameA).distanceTo(point(locked.links[index + 1], joint.frameB)) < 1e-6)
  })
})

test('dragging the end of a ball-jointed chain moves its link and keeps joints coincident', () => {
  const root = new TestComponent('root')
  const anchor = new TestComponent('anchor', root)
  const link = new TestComponent('link', root, new THREE.Vector3(1, 0, 0))
  const end = new TestComponent('end', root, new THREE.Vector3(3, 0, 0))
  root.assemblyJoints = [
    { type: 'fix', componentA: anchor.id, fixedWorld: new THREE.Matrix4() },
    { type: 'ball', componentA: anchor.id, componentB: link.id, frameA: at(0), frameB: at(-1) },
    { type: 'ball', componentA: link.id, componentB: end.id, frameA: at(1), frameB: at(-1) },
  ]

  solveAssembly(root, end, at(2.6, 1.2))

  assert.ok(point(anchor, at(0)).distanceTo(point(link, at(-1))) < 1e-3)
  assert.ok(point(link, at(1)).distanceTo(point(end, at(-1))) < 1e-3)
  assert.ok(point(link).y > 0.5)
})
