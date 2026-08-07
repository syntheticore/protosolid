import Serialize from './serialize.js'
import { Wire, Profile } from './geom3d.js'

export class Reference {
  constructor(item) {
    this.item = item
  }

  getItem() {
    return this.item
  }

  update(_tree) {}

  clone() {
    return new this.constructor(this.item)
  }
}

class TopoReference extends Reference {
  constructor(item, componentId, solidId, topoId) {
    super(item)
    this.componentId = componentId || item.solid.compound.componentId
    this.solidId = solidId || item.solid.id
    this.topoId = topoId || item.id
    // const root = window.oc.mainDoc.Main()
    // this.label = root.NewChild()
    // this.selector = new window.oc.oc.TNaming_Selector(this.label)
    // const geom = this.item.geom()
    // console.log(this.selector.Select_2(geom, false, false))
  }

  clone() {
    return new this.constructor(this.item, this.componentId, this.solidId, this.topoId)
  }

  dump() {
    return {
      componentId: this.componentId,
      solidId: this.solidId,
      topoId: this.topoId,
    }
  }

  static undump(dump) {
    return new this(null, dump.componentId, dump.solidId, dump.topoId)
  }
}

export class FaceReference extends TopoReference {
  update(tree) {
    const comp = tree.findChild(this.componentId)
    const solid = comp.compound.solids().find(solid => solid.id == this.solidId )
    const error = { type: 'error', msg: "Face reference was lost" }
    if(!solid) return error
    const face = solid.faces().find(face => face.isSame(this.topoId) )
    if(!face) return error
    this.item = face
  }
}
Serialize.register(FaceReference, 'FaceReference')


export class EdgeReference extends TopoReference {
  update(tree) {
    const comp = tree.findChild(this.componentId)
    const solid = comp.compound.solids().find(solid => solid.id == this.solidId )

    // const labelMap = new window.oc.oc.TDF_LabelMap_1()
    // this.selector.Solve(labelMap)
    // const ns = this.selector.NamedShape().get()
    // const shape = ns.Get()

    const edge = solid.edges().find(edge => edge.isSame(this.topoId) )
    // const edge = solid.edges().find(edge => edge.geom().IsSame(shape) )
    if(!edge) return { type: 'error', msg: "Edge reference was lost" }
    this.item = edge
  }
}
Serialize.register(EdgeReference, 'EdgeReference')


export class SolidReference extends Reference {
  constructor(item, componentId, solidId) {
    super(item)
    this.componentId = componentId || item.compound.componentId
    this.solidId = solidId || item.id
  }

  update(tree) {
    const comp = tree.findChild(this.componentId)
    const solid = comp.compound.solids().find(solid => solid.id == this.solidId )
    if(!solid) return { type: 'error', msg: "Solid reference was lost" }
    this.item = solid
  }

  dump() {
    return {
      componentId: this.componentId,
      solidId: this.solidId,
    }
  }

  static undump(dump) {
    return new this(null, dump.componentId, dump.solidId)
  }
}
Serialize.register(SolidReference, 'SolidReference')


export class CurveReference extends Reference {
  constructor(item, componentId, sketchId, isProjection, itemId) {
    super(item)
    this.componentId = componentId || item.sketch.component.id
    this.sketchId = sketchId || item.sketch.id
    this.isProjection = isProjection !== undefined ? isProjection : !!item.projection
    this.itemId = itemId || (this.isProjection ? item.projection.id : item.id)
  }

  update(tree) {
    const comp = tree.findChild(this.componentId)
    const sketch = comp.sketches.find(sketch => sketch.id == this.sketchId )
    const item = (this.isProjection ? sketch.projections : sketch.elements).find(item => item.id == this.itemId )
    this.item = this.isProjection ? item.geometry() : item
  }

  dump() {
    return {
      componentId: this.componentId,
      sketchId: this.sketchId,
      isProjection: this.isProjection,
      itemId: this.itemId,
    }
  }

  static undump(dump, context) {
    return new this(null, dump.componentId, dump.sketchId, dump.isProjection, dump.itemId)
  }
}
Serialize.register(CurveReference, 'CurveReference')


export class ProfileReference extends Reference {
  update(_tree) {
    return this.item.update()
  }

  clone() {
    return new ProfileReference(this.item.clone())
  }

  dump() {
    return {
      id: this.item.id,
      sketchId: this.item.sketch.id,
      rings: this.item.rings.map(wire => wire.segments.map(seg => seg.id) )
    }
  }

  static undump(dump, context) {
    const sketch = context.sketches[dump.sketchId]
    // Profile rings refer to the segments produced after intersecting sketch
    // elements (for example, `circle-id/0`), not necessarily to the original
    // elements stored on the sketch. Recreate those segments before resolving
    // the saved ring IDs.
    const elements = sketch.removeEmpties(sketch.elements)
    const segments = elements.flatMap(elem => elem.split(elements))
    const rings = dump.rings.map(ring => {
      const region = ring.map(segId => segments.find(elem => elem.id == segId))
      return new Wire(region)
    })
    const profile = new Profile(sketch, rings)
    profile.id = dump.id
    return new ProfileReference(profile)
  }
}
Serialize.register(ProfileReference, 'ProfileReference')


export class HelperReference extends Reference {
  constructor(item, componentId, helperId) {
    super(item)
    this.componentId = componentId || item.componentId
    this.helperId = helperId || item.id
  }

  update(tree) {
    const comp = tree.findChild(this.componentId)
    const error = { type: 'error', msg: "Helper reference was lost" }
    if(!comp) return error
    const helper = comp.helpers.find(helper => helper.id == this.helperId)
    if(!helper) return error
    this.item = helper
  }

  clone() {
    return new HelperReference(this.item, this.componentId, this.helperId)
  }

  dump() {
    return {
      componentId: this.item.componentId,
      helperId: this.item.id,
    }
  }

  static undump(dump) {
    return new HelperReference(null, dump.componentId, dump.helperId)
  }
}
Serialize.register(HelperReference, 'HelperReference')


// Can be HelperReference(PlaneHelper) or FaceReference
export class PlanarReference extends Reference {
  getItem() {
    const item = this.item.getItem()
    return item && item.getPlane()
  }

  update(tree) {
    return this.item.update(tree)
  }

  clone() {
    return new PlanarReference(this.item.clone())
  }

  static undump(dump) {
    return new PlanarReference(dump.item)
  }
}
Serialize.register(PlanarReference, 'PlanarReference')


export class AxialReference extends Reference {
  getItem() {
    const item = this.item.getItem()
    return item && item.getAxis()
  }

  update(tree) {
    return this.item.update(tree)
  }

  clone() {
    return new AxialReference(this.item.clone())
  }

  static undump(dump) {
    return new AxialReference(dump.item)
  }
}
Serialize.register(AxialReference, 'AxialReference')


export class PointReference extends Reference {
  getItem() {
    const item = this.item.getItem()
    return item && item.getPoint()
  }

  update(tree) {
    return this.item.update(tree)
  }

  clone() {
    return new PointReference(this.item.clone())
  }

  static undump(dump) {
    return new PointReference(dump.item)
  }
}
Serialize.register(PointReference, 'PointReference')
