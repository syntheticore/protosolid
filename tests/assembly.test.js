import assert from 'node:assert/strict'
import test from 'node:test'
import * as THREE from 'three'

import {
  alignJointWorld,
  solveAssembly,
  worldTransform,
} from '../js/core/assembly.js'
import { JointFeature } from '../js/core/features.js'
import { Face } from '../js/core/geom3d.js'


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

test('axis alignment removes radial offset while preserving axial slide', () => {
  const frameB = at(2, 0, 7)
  const aligned = alignJointWorld('axis', new THREE.Matrix4(), new THREE.Matrix4(), new THREE.Matrix4(), frameB)
  const attachment = new THREE.Vector3().setFromMatrixPosition(aligned.multiply(frameB))
  assert.ok(Math.hypot(attachment.x, attachment.y) < 1e-8)
  assert.ok(Math.abs(attachment.z - 7) < 1e-8)
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
