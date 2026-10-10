import assert from 'node:assert/strict'
import test from 'node:test'
import * as THREE from 'three'
import { withTopologyNaming } from '../js/core/topological-naming.js'
import Serialize from '../js/core/serialize.js'


export async function registerTopologyTests(backend) {
const { Compound, Profile, Wire } = await import('../js/core/geom3d.js')
const { Line, Circle } = await import('../js/core/geom2d.js')
const { EdgeReference, FaceReference, SolidReference, PatternInputReference } = await import('../js/core/references.js')
const { Feature, SolidFeature, PlaneFeature, SplitFeature, ChamferFeature, FilletFeature, PatternFeature, BooleanFeature, RemoveSolidsFeature } = await import('../js/core/features.js')
const { Timeline } = await import('../js/core/timeline.js')
const { Projection } = await import('../js/core/sketch.js')
const { FluidSimulation } = await import('../js/core/fluid.js')

function rectangle(width = 10, height = 10, prefix = 'base') {
  const points = [[0, 0], [width, 0], [width, height], [0, height]].map(p => new THREE.Vector3(...p, 0))
  const segments = ['south', 'east', 'north', 'west'].map((name, i) => {
    const segment = new Line(points[i], points[(i + 1) % points.length])
    segment.id = `${prefix}-${name}`
    return segment
  })
  return new Profile({ workplane: new THREE.Matrix4() }, [new Wire(segments)])
}

const tree = compound => ({ findChild: () => ({ compound }) })
const feature = id => ({ id })
const near = (a, b) => Math.abs(a - b) < 1e-7
const extrude = (owner, width = 10, depth = 10, reverse = false) => withTopologyNaming(owner, () => {
  try {
    const profile = rectangle(width)
    if(reverse) profile.rings = [new Wire(profile.rings[0].segments.slice().reverse().map(segment => {
      const reversed = new Line(...segment.endpoints().slice().reverse())
      reversed.id = segment.id
      return reversed
    }))]
    const result = profile.extrude('component', depth, owner.id).repair()
    result.restoreTopologySelections(owner)
    return result
  }
  catch(error) {
    if(typeof error == 'number') throw new Error(window.oc.oc.OCJS.getStandard_FailureData(error).GetMessageString())
    throw error
  }
})

test(`${backend}: references survive independent rebuild, dimension change, and save/reload`, () => {
  const owner = feature('extrusion')
  const original = extrude(owner)
  const face = original.solids()[0].faces().find(face => near(face.center().z, 10))
  const edge = face.solid.edges().find(edge => edge.connectedFaces().includes(face))
  const side = original.faces().find(face => near(face.center().x, 10))
  const references = [new FaceReference(face), new EdgeReference(edge), new SolidReference(face.solid), new FaceReference(side)]
  const saved = Serialize.stringify({ owner, references })
  const restored = Serialize.parse(saved)
  const rebuilt = extrude(restored.owner, 20, 15, true)
  rebuilt.cachedSolids.reverse()
  rebuilt.solids().forEach(solid => { solid.cachedFaces.reverse(); solid.cachedEdges.reverse() })
  for(const reference of restored.references) {
    assert.equal(reference.update(tree(rebuilt)), undefined)
    assert.ok(reference.getItem())
  }
  assert.ok(near(restored.references[0].getItem().center().z, 15))
  assert.ok(near(restored.references[3].getItem().center().x, 20))
  const moved = withTopologyNaming(feature('inserted-move'), () =>
    rebuilt.transform(new THREE.Matrix4().makeTranslation(5, 0, 0)).repair())
  for(const reference of restored.references) assert.equal(reference.update(tree(moved)), undefined)
  assert.ok(near(restored.references[0].getItem().center().x, 15))
})

test(`${backend}: picking from an incomplete feature preview binds the upstream cached shape`, () => {
  const owner = feature('extrusion')
  const upstream = extrude(owner)
  // The UI inserts an incomplete feature before its picker is used. Its
  // displayed tree is a clone of the upstream cache that will be replayed.
  const preview = upstream.cloneCached()
  const solid = preview.solids()[0]
  const face = solid.faces().find(face => near(face.center().z, 10))
  const edge = solid.edges().find(edge => edge.connectedFaces().includes(face))
  for(const ref of [new FaceReference(face), new EdgeReference(edge), new SolidReference(solid)]) {
    assert.equal(ref.update(tree(upstream)), undefined)
  }
})

test(`${backend}: viewport sections return compounds, retain upstream references and exclude transient picks`, async () => {
  const plane = new THREE.Matrix4().makeRotationY(Math.PI / 2).setPosition(5, 0, 0)
  assert.deepEqual(new Compound('component').halfCut(plane, false).solids(), [])
  const base = extrude(feature('base'))
  const initialVolume = base.volume()
  const solid = base.solids()[0]
  const bodyRef = new SolidReference(solid)
  const topRef = new FaceReference(solid.faces().find(face => near(face.center().z, 10)))
  const section = base.halfCut(plane, false)
    .halfCut(new THREE.Matrix4().makeTranslation(0, 0, 5), false)
  assert.equal(section.solids().length, 1)
  assert.equal(section.solids()[0].id, solid.id)
  assert.ok(Math.abs(section.volume() - 250) < 1e-7)
  for(const reference of [bodyRef, topRef]) assert.equal(reference.update(tree(section)), undefined)
  const displayedTop = section.solids()[0].faces().find(face => near(face.center().z, 10))
  assert.equal(new FaceReference(displayedTop).update(tree(base)), undefined)
  const { FacePickTool, EdgePickTool, SolidPickTool } = await import('../js/tools.js')
  const displayedSolid = section.solids()[0]
  for(const [cells, PickTool, Reference, type] of [
    [displayedSolid.faces(), FacePickTool, FaceReference, 'face'],
    [displayedSolid.edges(), EdgePickTool, EdgeReference, 'edge'],
  ]) {
    assert.ok(cells.some(item => !item.topology.origin.feature))
    assert.ok(cells.some(item => item.topology.origin.feature))
    for(const item of cells) {
      const viewport = { document: {}, renderer: {
        objectsAtScreen: () => [{ alcObject: item, alcTypes: [type] }],
      } }
      const picked = await new PickTool(null, viewport).getObject(null, true)
      if(item.topology.origin.feature) {
        assert.equal(picked, item)
        assert.equal(new Reference(picked).update(tree(base)), undefined)
      } else {
        assert.equal(picked, undefined)
        if(type == 'face') {
          assert.equal(await new SolidPickTool(null, viewport).getObject(null, true), displayedSolid)
        }
      }
    }
  }
  assert.equal(base.volume(), initialVolume, 'section rendering must leave the timeline geometry intact')
})

test(`${backend}: fluid validation resolves saved face selections without binding them or confusing them with overlap`, () => {
  const owner = feature('extrusion')
  const original = extrude(owner)
  const faces = original.solids()[0].faces()
  const simulation = new FluidSimulation('flow')
  simulation.inletFaces = [new FaceReference(faces.find(face => near(face.center().z, 0)))]
  simulation.outletFaces = [new FaceReference(faces.find(face => near(face.center().z, 10)))]
  const restored = Serialize.parse(Serialize.stringify({ owner, simulation }))
  const rebuilt = extrude(restored.owner, 20, 15)
  assert.equal(restored.simulation.resolve(tree(rebuilt), false), undefined)
  for(const reference of [...restored.simulation.inletFaces, ...restored.simulation.outletFaces]) {
    assert.equal(reference.getItem(), null, 'render validation must not bind face references')
  }
  restored.simulation.outletFaces = [restored.simulation.inletFaces[0].clone()]
  assert.equal(restored.simulation.resolve(tree(rebuilt), false), 'A face cannot be both an inlet and an outlet')
})

test(`${backend}: generated chamfer faces and shell faces rebind across independent operations`, () => {
  const baseOwner = feature('base'), chamferOwner = feature('chamfer'), shellOwner = feature('shell')
  const build = width => {
    const base = extrude(baseOwner, width)
    const top = base.solids()[0].faces().find(face => near(face.center().z, 10))
    const edge = top.solid.edges().find(edge => edge.connectedFaces().includes(top) && near(edge.center().y, 0))
    const chamfer = withTopologyNaming(chamferOwner, () => {
      const result = base.chamfer([edge], 0.5).repair()
      result.restoreTopologySelections(chamferOwner)
      return result
    })
    const opening = chamfer.solids()[0].faces().find(face => near(face.center().z, 10))
    const shell = withTopologyNaming(shellOwner, () => {
      const result = chamfer.shell([opening], -0.5, 'sharp').repair()
      result.restoreTopologySelections(shellOwner)
      return result
    })
    return { chamfer, shell }
  }
  const original = build(10)
  const generated = original.chamfer.solids()[0].faces().find(face => face.center().z > 9 && face.center().z < 10)
  const chamferRef = new FaceReference(generated)
  const shellRefs = original.shell.solids()[0].faces().map(face => new FaceReference(face))
  const rebuilt = build(12)
  assert.equal(chamferRef.update(tree(rebuilt.chamfer)), undefined)
  for(const ref of shellRefs) assert.equal(ref.update(tree(rebuilt.shell)), undefined)
})

test(`${backend}: projected sketch curves retain the projection identity when the source display ID changes`, () => {
  const owner = feature('extrusion')
  const original = extrude(owner)
  const top = original.solids()[0].faces().find(face => near(face.center().z, 10))
  const edge = top.solid.edges().find(edge => edge.connectedFaces().includes(top))
  const projection = new Projection(edge)
  projection.sketch = { workplane: new THREE.Matrix4(), component: { id: 'component' }, creator: {} }
  const target = compound => ({ findChild: () => ({ compound, localTransform: () => new THREE.Matrix4() }) })
  projection.update(target(original))
  assert.equal(projection.error, null)
  const curveId = projection.geometry().id
  const rebuilt = extrude(owner, 12)
  projection.itemRef.update(target(rebuilt))
  projection.itemRef.getItem().id = 'different-viewport-object-id'
  projection.update(target(rebuilt))
  assert.equal(projection.error, null)
  assert.equal(projection.geometry().id, curveId)
})

test(`${backend}: loft, sweep, revolution and circular extrusion seed selectable construction roles`, () => {
  const build = (owner, type, size) => withTopologyNaming(owner, () => {
    let result
    if(type == 'circle' || type == 'torus') {
      const circle = new Circle(new THREE.Vector3(), size)
      circle.id = 'circle'
      const profile = new Profile({ workplane: new THREE.Matrix4() }, [new Wire([circle])])
      if(type == 'torus') profile.sketch.workplane.makeTranslation(30, 0, 0)
      result = type == 'circle' ? profile.extrude('component', 10, owner.id) :
        profile.revolve('component', new THREE.Matrix4().makeRotationX(Math.PI / 2), 2 * Math.PI, owner.id)
    } else if(type == 'loft') {
      const other = rectangle(size, size, 'end')
      other.sketch.workplane.makeTranslation(0, 0, 10)
      result = rectangle().loft('component', [other], true)
    } else if(type == 'sweep') {
      const rail = new Line(new THREE.Vector3(), new THREE.Vector3(0, 20, 0))
      rail.id = 'rail'
      rail.sketch = { workplane: new THREE.Matrix4().makeRotationX(Math.PI / 2) }
      result = rectangle(size).sweep('component', rail, [0, 1])
    } else {
      const profile = rectangle(size)
      profile.sketch.workplane.makeTranslation(20, 0, 0)
      const axis = new THREE.Matrix4().makeRotationX(Math.PI / 2)
      result = profile.revolve('component', axis, Math.PI, owner.id)
    }
    result = result.repair()
    result.restoreTopologySelections(owner)
    return result
  })
  for(const type of ['circle', 'loft', 'sweep', 'revolve', 'torus']) {
    const owner = feature(type)
    const original = build(owner, type, 10)
    const solid = original.solids()[0]
    // Capture every face and edge to expose missing generator correspondence,
    // including periodic seams and generator cap boundaries.
    const refs = [...solid.faces().map(face => new FaceReference(face)),
      ...solid.edges().filter(edge => edge.isSemantic()).map(edge => new EdgeReference(edge)), new SolidReference(solid)]
    const saved = Serialize.parse(Serialize.stringify({ owner, refs }))
    const rebuilt = build(saved.owner, type, 12)
    for(const ref of saved.refs) assert.equal(ref.update(tree(rebuilt)), undefined, type)
  }
})

test(`${backend}: limited and through-all extrusions retain semantic sides and caps`, () => {
  const supportOwner = feature('support')
  for(const mode of ['limited', 'through-all']) {
    const owner = feature(mode)
    const build = depth => withTopologyNaming(owner, () => {
      const support = extrude(supportOwner, 20, depth)
      const circle = new Circle(new THREE.Vector3(5, 5, 0), 2)
      circle.id = 'hole-circle'
      const profile = new Profile({ workplane: new THREE.Matrix4() }, [new Wire([circle])])
      const top = support.solids()[0].faces().find(face => near(face.center().z, depth))
      const result = (mode == 'limited' ? profile.extrudeUpToFace('component', top, owner.id) :
        profile.extrudeThroughAll('component', support, owner.id)).repair()
      result.restoreTopologySelections(owner)
      return result
    })
    const original = build(10)
    const refs = original.solids().flatMap(solid => [
      ...solid.faces().map(face => new FaceReference(face)),
      ...solid.edges().filter(edge => edge.isSemantic()).map(edge => new EdgeReference(edge)), new SolidReference(solid),
    ])
    const rebuilt = build(12)
    for(const ref of refs) assert.equal(ref.update(tree(rebuilt)), undefined, mode)
  }
})

test(`${backend}: selected split faces and edges retain their branch after rebuild and output permutation`, () => {
  const baseOwner = feature('base'), toolOwner = feature('tool'), cutOwner = feature('cut')
  const build = width => {
    const base = extrude(baseOwner, width)
    const tool = withTopologyNaming(toolOwner, () => rectangle(2, 20, 'tool')
      .extrude('component', 20, toolOwner.id).transform(new THREE.Matrix4().makeTranslation(4, -5, -5)).repair())
    return withTopologyNaming(cutOwner, () => {
      const cut = base.boolean(tool, 'cut').repair()
      cut.restoreTopologySelections(cutOwner)
      return cut
    })
  }
  const original = build(10)
  // Pick after a later operation, then omit that operation during rebuild.
  // The branch binding still belongs to the cut that actually split it.
  const displayed = withTopologyNaming(feature('temporary-move'), () =>
    original.transform(new THREE.Matrix4().makeTranslation(30, 0, 0)))
  const face = displayed.solids().flatMap(solid => solid.faces()).find(face => near(face.center().z, 10) && face.center().x < 34)
  const edge = face.solid.edges().find(edge => edge.connectedFaces().includes(face) && near(edge.center().y, 0))
  const faceRef = new FaceReference(face), edgeRef = new EdgeReference(edge)
  for(const reference of [faceRef, edgeRef]) assert.equal(reference.update(tree(original)), undefined)
  const saved = Serialize.stringify({ faceRef, edgeRef, cutOwner })
  const restored = Serialize.parse(saved)
  Object.assign(cutOwner, restored.cutOwner)
  const rebuilt = build(14)
  rebuilt.cachedSolids.reverse()
  rebuilt.solids().forEach(solid => { solid.cachedFaces.reverse(); solid.cachedEdges.reverse() })
  for(const reference of [restored.faceRef, restored.edgeRef]) {
    assert.equal(reference.update(tree(rebuilt)), undefined)
    assert.ok(reference.getItem().center().x < 4)
  }
})

test(`${backend}: a selected split region is deleted rather than rebound to its sole surviving sibling`, () => {
  const baseOwner = feature('base'), toolOwner = feature('tool'), cutOwner = feature('cut')
  const base = extrude(baseOwner)
  const tool = withTopologyNaming(toolOwner, () => rectangle(2, 20, 'tool').extrude('component', 20, toolOwner.id)
    .transform(new THREE.Matrix4().makeTranslation(4, -5, -5)))
  const cut = withTopologyNaming(cutOwner, () => base.boolean(tool, 'cut').repair())
  const left = cut.solids().flatMap(solid => solid.faces()).find(face => near(face.center().z, 10) && face.center().x < 4)
  const reference = new FaceReference(left)
  const rebuilt = withTopologyNaming(cutOwner, () => {
    const all = extrude(baseOwner).boolean(tool, 'cut').repair()
    const right = all.solids().find(solid => solid.center().x > 6)
    const result = all.subset([right]).cloneCached()
    result.restoreTopologySelections(cutOwner)
    return result
  })
  assert.equal(reference.update(tree(rebuilt))?.type, 'error')
  assert.equal(reference.getItem(), null)
})

class BoxFeature extends Feature {
  constructor(document) {
    super(document, true, 'Box fixture', { width: { type: 'length' }, x: { type: 'length' } })
    this.width = 10
    this.x = 0
  }
  updateFeature(tree) {
    const compound = rectangle(this.width, 10, this.id).extrude(this.componentId, 10, this.id)
      .transform(new THREE.Matrix4().makeTranslation(this.x, 0, 0))
    const component = tree.findChild(this.componentId)
    component.compound = component.compound.boolean(compound, 'create')
  }
}
Serialize.register(BoxFeature, 'NamingBoxFixture')

function documentFixture() {
  const document = { timeline: new Timeline(), selection: { items: [] } }
  document.top = at => document.timeline.tree(at)
  document.activeComponent = document.top()
  return document
}

class TranslateVisibleBodiesFeature extends SolidFeature {
  constructor(document) {
    super(document, false, 'Translate visible bodies fixture', { distance: { type: 'length' } })
    this.distance = 30
  }
  updateFeature(tree) {
    const component = tree.findChild(this.componentId)
    component.compound = component.compound.transform(new THREE.Matrix4().makeTranslation(0, this.distance, 0))
  }
}
Serialize.register(TranslateVisibleBodiesFeature, 'NamingTranslateVisibleBodiesFixture')

test(`${backend}: hidden split bodies retain visibility, downstream scope and patterned references after an earlier chamfer`, () => {
  const document = documentFixture()
  const box = new BoxFeature(document)
  box.x = -5
  document.timeline.insertFeature(box, false)
  document.timeline.regenerate(0, 1)
  const split = new SplitFeature(document)
  const plane = document.top().helpers.find(helper => helper.id.endsWith('/YZ')).planarReference()
  split.plane = () => plane
  document.timeline.insertFeature(split, false)
  document.timeline.regenerate(1, 2)
  const bodies = document.top().compound.solids()
  const hiddenId = bodies.find(solid => solid.center().x < 0).id
  const visibleId = bodies.find(solid => solid.center().x > 0).id
  document.top().creator.itemsHidden[hiddenId] = true
  const translate = new TranslateVisibleBodiesFeature(document)
  translate.visibilities = { [document.top().id]: {
    compHidden: false, itemsHidden: { ...document.top().creator.itemsHidden },
  } }
  document.timeline.insertFeature(translate, false)

  const check = doc => {
    const move = doc.timeline.features.find(feature => feature instanceof TranslateVisibleBodiesFeature)
    const component = doc.top(move)
    const hidden = component.compound.solids().find(solid => solid.center().x < 0)
    const visible = component.compound.solids().find(solid => solid.center().x > 0)
    assert.equal(hidden.id, hiddenId)
    assert.equal(visible.id, visibleId)
    assert.equal(component.isItemHidden(hidden.id), true)
    assert.equal(move.isSolidVisible(component, hidden), false)
    assert.equal(move.isSolidVisible(component, visible), true)
    assert.ok(Math.abs(hidden.center().y - 5) < 1e-7, 'the excluded half must not be translated')
    const splitFeature = doc.timeline.features.find(feature => feature instanceof SplitFeature)
    const upstream = doc.top(splitFeature).compound.solids().find(solid => solid.id == visibleId)
    assert.ok(Math.abs(visible.center().y - upstream.center().y - move.distance) < 1e-7,
      'the visible half must be translated')
  }
  document.timeline.regenerate(2, 3)
  check(document)
  document.timeline.regenerate(0, 3)
  check(document)
  box.width = 12
  document.timeline.regenerate(0, 3)
  check(document)

  const pattern = new PatternFeature(document)
  const input = PatternInputReference.fromSolid(document.top().compound.solids().find(solid => solid.id == hiddenId))
  pattern.inputs = () => [input]
  pattern.uCount = 1
  pattern.vCount = 2
  pattern.vOffset = 40
  pattern.operation = 'create'
  document.timeline.insertFeature(pattern, false)
  document.timeline.regenerate(3, 4)
  assert.equal(pattern.error, undefined)
  const copy = document.top().compound.solids().find(solid => solid.center().y > 40)
  const copyId = copy.id
  const copyRef = new SolidReference(copy)
  const checkCopy = doc => {
    assert.equal(doc.timeline.features.find(feature => feature instanceof PatternFeature).error, undefined)
    assert.equal(copyRef.update(doc.top()), undefined)
    assert.equal(copyRef.getItem().id, copyId)
    assert.ok(copyRef.getItem().center().x < 0)
  }

  const edge = document.top(box).compound.solids()[0].edges().find(edge =>
    near(edge.center().x, 7) && near(edge.center().y, 0))
  const chamfer = new ChamferFeature(document)
  const chamferRef = new EdgeReference(edge)
  chamfer.edges = () => [chamferRef]
  chamfer.distance = 0.5
  document.timeline.marker = 1
  document.timeline.insertFeature(chamfer, false)
  document.timeline.marker = 5
  document.timeline.regenerate(1, 5)
  assert.equal(chamfer.error, undefined)
  check(document)
  checkCopy(document)
  document.timeline.regenerate(0, 5)
  check(document)
  checkCopy(document)
  const reloaded = documentFixture()
  reloaded.timeline = Serialize.parse(Serialize.stringify(document.timeline), { document: reloaded })
  reloaded.timeline.regenerate(0, 5)
  check(reloaded)
  checkCopy(reloaded)

  document.timeline.removeFeature(chamfer)
  document.timeline.regenerate(1, 4)
  check(document)
  checkCopy(document)

  split.keep = 'left'
  document.timeline.regenerate(1, 4)
  assert.equal(document.top(translate).compound.solids().length, 1)
  const remaining = document.top(translate).compound.solids()[0]
  assert.equal(remaining.id, hiddenId)
  assert.equal(document.top().isItemHidden(remaining.id), true)
  assert.ok(Math.abs(remaining.center().y - 5) < 1e-7)
  checkCopy(document)

  split.keep = 'right'
  document.timeline.regenerate(1, 4)
  assert.equal(input.update(document.top(split))?.type, 'error', 'the deleted branch must not retarget its sibling')
  assert.equal(pattern.error?.type, 'error')
})

test(`${backend}: deleting an unrelated chamfer preserves a downstream fillet and body pattern, including reload`, () => {
  const document = documentFixture()
  const box = new BoxFeature(document)
  document.timeline.insertFeature(box, false)
  document.timeline.regenerate(0, 1)
  const southTop = document.top().compound.solids()[0].edges().find(edge =>
    near(edge.center().y, 0) && near(edge.center().z, 10))
  const chamfer = new ChamferFeature(document)
  const chamferRef = new EdgeReference(southTop)
  chamfer.edges = () => [chamferRef]
  chamfer.distance = 0.5
  document.timeline.insertFeature(chamfer, false)
  document.timeline.regenerate(1, 2)
  assert.equal(chamfer.error, undefined)
  const northTop = document.top().compound.solids()[0].edges().find(edge =>
    near(edge.center().y, 10) && near(edge.center().z, 10))
  const fillet = new FilletFeature(document)
  const filletRef = new EdgeReference(northTop)
  fillet.edges = () => [filletRef]
  fillet.radius = 0.5
  document.timeline.insertFeature(fillet, false)
  document.timeline.regenerate(2, 3)
  assert.equal(fillet.error, undefined)
  const input = PatternInputReference.fromSolid(document.top().compound.solids()[0])
  const pattern = new PatternFeature(document)
  pattern.inputs = () => [input]
  pattern.uOffset = 30
  pattern.uCount = 2
  pattern.vCount = 1
  pattern.operation = 'create'
  document.timeline.insertFeature(pattern, false)
  document.timeline.regenerate(3, 4)
  assert.equal(pattern.error, undefined)

  document.timeline.removeFeature(chamfer)
  document.timeline.regenerate(1, 3)
  assert.equal(fillet.error, undefined)
  assert.equal(pattern.error, undefined)
  assert.equal(document.top().compound.solids().length, 2)
  assert.ok(near(fillet.edges()[0].getItem().center().y, 10))
  const reloaded = documentFixture()
  reloaded.timeline = Serialize.parse(Serialize.stringify(document.timeline), { document: reloaded })
  reloaded.timeline.regenerate(0, 3)
  assert.equal(reloaded.timeline.features[1].error, undefined)
  assert.equal(reloaded.timeline.features[2].error, undefined)
  assert.equal(reloaded.top().compound.solids().length, 2)
})

test(`${backend}: production timeline keeps a chamfer when a feature is inserted before its producer, including reload`, () => {
  const document = documentFixture()
  const box = new BoxFeature(document)
  document.timeline.insertFeature(box, false)
  document.timeline.regenerate(0, 1)
  const compound = document.top().compound
  const top = compound.solids()[0].faces().find(face => near(face.center().z, 10))
  const target = top.solid.edges().find(edge => edge.connectedFaces().includes(top))
  const chamfer = new ChamferFeature(document)
  const reference = new EdgeReference(target)
  chamfer.edges = () => [reference]
  chamfer.distance = 0.5
  document.timeline.insertFeature(chamfer, false)
  document.timeline.regenerate(1, 2)
  assert.equal(chamfer.error, undefined)
  const inserted = new BoxFeature(document)
  inserted.x = 40
  document.timeline.marker = 0
  document.timeline.insertFeature(inserted, false)
  document.timeline.marker = 3
  box.width = 20
  document.timeline.regenerate(0, 3)
  assert.equal(chamfer.error, undefined)
  assert.equal(document.top().compound.solids().length, 2)
  const saved = Serialize.stringify(document.timeline)
  const reloaded = documentFixture()
  reloaded.timeline = Serialize.parse(saved, { document: reloaded })
  reloaded.timeline.regenerate(0, 3)
  const restoredChamfer = reloaded.timeline.features[2]
  assert.equal(restoredChamfer.error, undefined)
  assert.ok(restoredChamfer.edges()[0].getItem().center().x < 30, 'must select the original body, not the inserted body')
  assert.equal(reloaded.top().compound.solids().length, 2)
})

test(`${backend}: pattern selections keep their grid cell when an earlier row is added`, () => {
  const document = documentFixture()
  const box = new BoxFeature(document)
  document.timeline.insertFeature(box, false)
  document.timeline.regenerate(0, 1)
  const source = document.top().compound.solids()[0]
  const input = PatternInputReference.fromSolid(source)
  assert.equal(input.matches(source), true)
  const pattern = new PatternFeature(document)
  pattern.inputs = () => [input]
  pattern.uOffset = 30
  pattern.vOffset = 30
  pattern.uCount = 2
  pattern.vCount = 2
  pattern.operation = 'create'
  document.timeline.insertFeature(pattern, false)
  document.timeline.regenerate(1, 2)
  assert.equal(pattern.error, undefined)
  const solid = document.top().compound.solids().find(solid => solid.center().x > 30 && solid.center().y < 30)
  const ref = new SolidReference(solid)
  const top = solid.faces().find(face => near(face.center().z, 10))
  const faceRef = new FaceReference(top)
  pattern.vCount = 3
  document.timeline.regenerate(1, 2)
  assert.equal(pattern.error, undefined)
  for(const selected of [ref, faceRef]) {
    assert.equal(selected.update(document.top()), undefined)
    assert.ok(selected.getItem().center().x > 30)
    assert.ok(selected.getItem().center().y < 30)
  }
  const reloaded = documentFixture()
  reloaded.timeline = Serialize.parse(Serialize.stringify(document.timeline), { document: reloaded })
  reloaded.timeline.regenerate(0, 2)
  for(const selected of [ref, faceRef]) assert.equal(selected.update(reloaded.top()), undefined)
})

test(`${backend}: split-side identities survive an empty side, its return, and reload without retargeting a pattern`, () => {
  const document = documentFixture()
  const add = feature => {
    const index = document.timeline.features.length
    document.timeline.insertFeature(feature, false)
    document.timeline.regenerate(index, index + 1)
    assert.notEqual(feature.error?.type, 'error')
  }
  add(new BoxFeature(document))
  const plane = new PlaneFeature(document)
  const base = document.top().helpers.find(helper => helper.id.endsWith('/XY')).planarReference()
  plane.base = () => base
  plane.offset = 5
  add(plane)
  const split = new SplitFeature(document)
  const planeRef = document.top().helpers.find(helper => helper.id == plane.planeId).planarReference()
  split.plane = () => planeRef
  split.keep = 'both'
  add(split)

  const halves = document.top().compound.solids()
  const left = halves.find(solid => solid.center().z < 5)
  const right = halves.find(solid => solid.center().z > 5)
  const leftId = left.id, rightId = right.id
  const leftRef = new SolidReference(left), rightRef = new SolidReference(right)
  const top = right.faces().find(face => near(face.center().z, 10))
  const faceRef = new FaceReference(top)
  const edgeRef = new EdgeReference(right.edges().find(edge => edge.connectedFaces().includes(top)))
  document.top().creator.itemsHidden[leftId] = true
  const translate = new TranslateVisibleBodiesFeature(document)
  translate.visibilities = { [document.top().id]: {
    compHidden: false, itemsHidden: { [leftId]: true },
  } }
  add(translate)
  const pattern = new PatternFeature(document)
  const input = PatternInputReference.fromSolid(right)
  pattern.inputs = () => [input]
  pattern.uCount = 2
  pattern.vCount = 1
  pattern.uOffset = 30
  pattern.operation = 'create'
  add(pattern)
  const copyId = document.top().compound.solids().find(solid => solid.center().x > 30).id

  const check = (doc, refs, offset) => {
    const [, plane, split, translate, pattern] = doc.timeline.features
    plane.offset = offset
    doc.timeline.regenerate(0, 5)
    const splitTree = doc.top(split)
    const leftExists = offset > 0, rightExists = offset < 10
    for(const [reference, exists] of [[refs.leftRef, leftExists], [refs.rightRef, rightExists],
      [refs.faceRef, rightExists], [refs.edgeRef, rightExists]]) {
      assert.equal(reference.update(splitTree)?.type, exists ? undefined : 'error')
      assert.equal(!!reference.getItem(), exists)
    }
    assert.equal(splitTree.compound.solids().length, leftExists && rightExists ? 2 : 1)
    if(leftExists) {
      assert.equal(refs.leftRef.getItem().id, leftId)
      assert.equal(splitTree.isItemHidden(leftId), true)
      const hidden = doc.top(translate).compound.solids().find(solid => solid.id == leftId)
      assert.ok(Math.abs(hidden.center().y - 5) < 1e-7, 'the hidden survivor must remain outside the move scope')
    }
    if(rightExists) {
      assert.equal(refs.rightRef.getItem().id, rightId)
      assert.equal(pattern.error, undefined)
      const copy = doc.top().compound.solids().find(solid => solid.center().x > 30)
      assert.equal(copy.id, copyId)
      assert.ok(Math.abs(copy.volume() - (offset < 0 ? 1000 : 500)) < 1e-7)
    } else {
      assert.equal(pattern.error?.type, 'error')
      assert.equal(doc.top().compound.solids().length, 1, 'a deleted branch must not pattern the whole body')
    }
  }
  const refs = { leftRef, rightRef, faceRef, edgeRef }
  for(const offset of [5, 20, 5, -5, 5, 20]) check(document, refs, offset)
  const reloaded = documentFixture()
  const saved = Serialize.parse(Serialize.stringify({ timeline: document.timeline, ...refs }), { document: reloaded })
  reloaded.timeline = saved.timeline
  for(const offset of [20, 5, -5, 5]) check(reloaded, saved, offset)
})

test(`${backend}: a split first evaluated outside the body assigns the same side identity when the other side appears`, () => {
  const baseOwner = feature('base'), splitOwner = feature('split')
  const build = offset => withTopologyNaming(splitOwner, () => {
    const result = extrude(baseOwner).split(new THREE.Matrix4().makeTranslation(0, 0, offset), 'both').repair()
    result.restoreTopologySelections(splitOwner)
    return result
  })
  const outside = build(20)
  const id = outside.solids()[0].id
  const reference = new SolidReference(outside.solids()[0])
  const inside = build(5)
  assert.equal(reference.update(tree(inside)), undefined)
  assert.equal(reference.getItem().id, id)
  assert.ok(reference.getItem().center().z < 5)
  assert.equal(reference.update(tree(build(-5)))?.type, 'error')
  assert.equal(reference.update(tree(build(20))), undefined)
  assert.equal(reference.getItem().id, id)
})

test(`${backend}: boolean operand roles preserve target bodies and boundary faces without reviving a consumed tool`, () => {
  const document = documentFixture()
  const target = new BoxFeature(document), tool = new BoxFeature(document)
  tool.x = 5
  for(const feature of [target, tool]) document.timeline.insertFeature(feature, false)
  document.timeline.regenerate(0, 2)
  const bodies = document.top().compound.solids()
  const targetBody = bodies.find(solid => solid.center().x < 10)
  const toolBody = bodies.find(solid => near(solid.center().x, 10))
  const targetRef = new SolidReference(targetBody), toolRef = new SolidReference(toolBody)
  const targetId = targetBody.id
  const faceRef = new FaceReference(toolBody.faces().find(face => near(face.center().x, 5)))
  const boolean = new BooleanFeature(document)
  boolean.operation = 'cut'
  boolean.tools = () => [toolRef]
  document.timeline.insertFeature(boolean, false)
  const remove = new RemoveSolidsFeature(document)
  remove.solids = () => [toolRef]
  document.timeline.insertFeature(remove, false)

  for(const keep of [false, true, false]) {
    boolean.keepTools = keep
    document.timeline.regenerate(0, 4)
    assert.equal(boolean.error, undefined)
    const result = document.top(boolean)
    assert.equal(targetRef.update(result), undefined)
    assert.equal(targetRef.getItem().id, targetId)
    assert.ok(Math.abs(targetRef.getItem().volume() - 500) < 1e-7)
    assert.ok(targetRef.getItem().topology.ancestors.includes(toolBody.topology.identity),
      'tool geometry contributes ancestry without continuing the tool body')
    assert.equal(toolRef.update(result)?.type, keep ? undefined : 'error')
    if(keep) assert.ok(Math.abs(toolRef.getItem().volume() - 1000) < 1e-7)
    else {
      assert.equal(toolRef.getItem(), null)
      assert.equal(faceRef.update(result), undefined, 'the consumed tool can still contribute a boundary face')
      assert.equal(remove.error?.type, 'error')
    }
    assert.equal(document.top().compound.solids().length, 1)
  }

  const reloaded = documentFixture()
  reloaded.timeline = Serialize.parse(Serialize.stringify(document.timeline), { document: reloaded })
  reloaded.timeline.regenerate(0, 4)
  assert.equal(reloaded.timeline.features[3].error?.type, 'error')
  assert.equal(reloaded.top().compound.solids().length, 1)
  for(const operation of ['join', 'intersect']) {
    boolean.operation = operation
    document.timeline.regenerate(0, 3)
    assert.equal(targetRef.update(document.top(boolean)), undefined)
    assert.equal(toolRef.update(document.top(boolean)), undefined)
    assert.equal(targetRef.getItem(), toolRef.getItem(), 'both operands continue into the merged result')
  }
})


}
