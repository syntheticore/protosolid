import Serialize from './serialize.js'
import { Wire, Profile } from './geom3d.js'

export class Reference {
  constructor(item) {
    this.item = item
  }

  getItem() {
    return this.item
  }

  getItems() {
    const item = this.getItem()
    return item ? [item] : []
  }

  update(_tree) {}

  clone() {
    return new this.constructor(this.item)
  }
}

export class ComponentReference extends Reference {
  constructor(item, componentId) {
    super(item)
    this.componentId = componentId || item.id
  }

  update(tree) {
    const component = tree.findChild(this.componentId)
    if(!component) return { type: 'error', msg: 'Component reference was lost' }
    this.item = component
  }

  clone() { return new ComponentReference(this.item, this.componentId) }
  dump() { return { componentId: this.componentId } }
  static undump(dump) { return new ComponentReference(null, dump.componentId) }
}
Serialize.register(ComponentReference, 'ComponentReference')

export class JointReference extends Reference {
  constructor(item, jointId) {
    super(item)
    this.jointId = jointId || item.id
  }

  update(tree) {
    const joint = (tree.assemblyJoints || []).find(joint => joint.id == this.jointId)
    if(!joint) return { type: 'error', msg: 'Joint reference was lost' }
    this.item = joint
  }

  clone() { return new JointReference(this.item, this.jointId) }
  matches(item) { return this.jointId == item.id }
  dump() { return { jointId: this.jointId } }
  static undump(dump) { return new JointReference(null, dump.jointId) }
}
Serialize.register(JointReference, 'JointReference')

class TopoReference extends Reference {
  constructor(item, componentId, selection) {
    super(item)
    const compound = item && (item.solid ? item.solid.compound : item.compound)
    this.componentId = componentId || compound.componentId
    this.selection = selection || compound.topologySelection(item)
    this.items = item ? [item] : []
  }

  clone() {
    return new this.constructor(this.item, this.componentId, this.selection)
  }

  dump() {
    return {
      componentId: this.componentId,
      selection: this.selection,
    }
  }

  static undump(dump) {
    return new this(null, dump.componentId, dump.selection)
  }

  getItems() { return this.items }

  updateTopology(tree, label) {
    const compound = tree.findChild(this.componentId)?.compound
    const resolution = compound ? compound.resolveTopology(this.selection) : { status: 'deleted', items: [] }
    this.items = resolution.items
    this.item = this.items.length == 1 ? this.items[0] : null
    if(resolution.status != 'resolved') return {
      type: 'error',
      msg: resolution.status == 'ambiguous' ? `${label} reference is ambiguous` : `${label} reference was deleted`,
    }
  }
}

export class FaceReference extends TopoReference {
  update(tree) {
    return this.updateTopology(tree, 'Face')
  }
}
Serialize.register(FaceReference, 'FaceReference')


export class EdgeReference extends TopoReference {
  update(tree) {
    return this.updateTopology(tree, 'Edge')
  }
}
Serialize.register(EdgeReference, 'EdgeReference')


export class SolidReference extends TopoReference {
  update(tree) {
    return this.updateTopology(tree, 'Solid')
  }
}
Serialize.register(SolidReference, 'SolidReference')


// Pattern inputs may point either at a body in the model or at a feature in
// the timeline. Keeping this as a reference (instead of storing geometry)
// makes saved patterns regenerate against the latest upstream result.
export class PatternInputReference extends Reference {
  constructor(item, document, featureId, solidReference, componentReference) {
    super(item)
    this.document = document
    this.featureId = featureId
    this.solidReference = solidReference
    this.componentReference = componentReference
  }

  static fromSolid(solid) {
    return new PatternInputReference(solid, null, null, solid.reference())
  }

  static fromFeature(feature) {
    return new PatternInputReference(feature.previewBody, feature.document, feature.id)
  }

  static fromComponent(component) {
    return new PatternInputReference(component, null, null, null, component.componentReference())
  }

  static fromItem(item) {
    const type = item?.typename?.()
    if(type == 'Component') return PatternInputReference.fromComponent(item)
    if(type == 'Solid') return PatternInputReference.fromSolid(item)
    if(item?.patternReference) return item.patternReference()
  }

  update(tree) {
    if(this.componentReference) {
      const error = this.componentReference.update(tree)
      this.item = this.componentReference.getItem()
      return error
    }
    if(this.solidReference) {
      const error = this.solidReference.update(tree)
      this.item = this.solidReference.getItem()
      return error
    }

    const feature = this.document.timeline.features.find(feature => feature.id == this.featureId)
    if(!feature) return { type: 'error', msg: 'Pattern input feature was lost' }

    // Additive features expose their tool body. For modifying features, use
    // the affected component's current result so every timeline feature is a
    // valid pattern input.
    this.item = feature.previewBody || tree.findChild(feature.componentId)?.compound
    if(!this.item || !this.item.geom) {
      return { type: 'error', msg: 'Pattern input feature has no solid result' }
    }
  }

  clone() {
    return new PatternInputReference(
      this.item,
      this.document,
      this.featureId,
      this.solidReference && this.solidReference.clone(),
      this.componentReference && this.componentReference.clone(),
    )
  }

  matches(item) {
    return this.featureId ? this.featureId == item.id :
      this.componentReference ? this.componentReference.componentId == item.id :
      this.solidReference?.componentId == item.compound?.componentId &&
        item.topology?.names.includes(this.solidReference.selection.identity)
  }

  dump() {
    return {
      featureId: this.featureId,
      solidReference: this.solidReference,
      componentReference: this.componentReference,
    }
  }

  static undump(dump, context) {
    return new PatternInputReference(null, context.document, dump.featureId, dump.solidReference, dump.componentReference)
  }
}
Serialize.register(PatternInputReference, 'PatternInputReference')


export class CurveReference extends Reference {
  constructor(item, componentId, sketchId, isProjection, itemId) {
    super(item)
    this.componentId = componentId || item.sketch.component.id
    this.sketchId = sketchId || item.sketch.id
    this.isProjection = isProjection !== undefined ? isProjection : !!item.projection
    this.itemId = itemId || (this.isProjection ? item.projection.id : item.id)
  }

  clone() {
    // Deserialized references are intentionally unresolved until update().
    // Preserve their stable IDs instead of asking a null item for its sketch.
    return new CurveReference(this.item, this.componentId, this.sketchId, this.isProjection, this.itemId)
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


export class SketchOriginReference extends Reference {
  constructor(item, componentId, sketchId) {
    super(item)
    this.componentId = componentId || item.sketch.component?.id || item.sketch.creator.componentId
    this.sketchId = sketchId || item.sketch.id
  }

  update(tree) {
    const comp = tree.findChild(this.componentId)
    const sketch = comp && comp.sketches.find(sketch => sketch.id == this.sketchId)
    if(!sketch) return { type: 'error', msg: 'Sketch origin reference was lost' }
    this.item = sketch.origin()
  }

  clone() {
    return new SketchOriginReference(this.item, this.componentId, this.sketchId)
  }

  dump() {
    return {
      componentId: this.componentId,
      sketchId: this.sketchId,
    }
  }

  static undump(dump) {
    return new SketchOriginReference(null, dump.componentId, dump.sketchId)
  }
}
Serialize.register(SketchOriginReference, 'SketchOriginReference')


export class ProfileReference extends Reference {
  update(_tree) {
    return this.item.update()
  }

  clone() {
    return new ProfileReference(this.item.clone())
  }

  matches(item) {
    if(!this.item || !item || this.item.sketch.id != item.sketch.id) return false

    const ringIds = profile => (profile.serializedRingIds ||
      profile.rings.map(wire => wire.segments.map(segment => segment.id))
    ).map(ring => [...ring].sort() )
      .sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)))

    return JSON.stringify(ringIds(this.item)) == JSON.stringify(ringIds(item))
  }

  dump() {
    return {
      id: this.item.id,
      sketchId: this.item.sketch.id,
      rings: this.item.serializedRingIds ||
        this.item.rings.map(wire => wire.segments.map(seg => seg.id) )
    }
  }

  static undump(dump, context) {
    const sketch = context.sketches[dump.sketchId]
    // Profile rings refer to the segments produced after intersecting sketch
    // elements (for example, `circle-id/0`), not necessarily to the original
    // elements stored on the sketch. Recreate those segments before resolving
    // the saved ring IDs.
    const elements = sketch.profileElements()
    const segments = elements.flatMap(elem => elem.split(elements))
    const regions = dump.rings.map(ring =>
      ring.map(segId => segments.find(elem => elem.id == segId))
    )
    const resolved = !regions.some(region => region.some(seg => !seg))
    const rings = resolved ? regions.map(region => new Wire(region)) : []
    const profile = new Profile(sketch, rings)
    if(!resolved) profile.serializedRingIds = dump.rings
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
