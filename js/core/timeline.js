import Serialize from './serialize.js'
import { Component, ComponentDefinition } from './component.js'
import { CreateComponentFeature, Feature } from './features.js'
import { arrayRange, makeColor } from './utils.js'


export class Timeline {
  constructor(baseCompDef) {
    this.baseCompDef = baseCompDef || new ComponentDefinition('Main Assembly', 'hsl(47.88, 100%, 59.22%)')
    const baseComp = new Component(null, 'id-0')
    baseComp.creator = this.baseCompDef
    this.cache = [baseComp]
    this.features = []
    this.marker = 0
    this.last_change_index = 0
    this.last_eval_index = 0
    // removal_modifications: Vec<CompRef>,
  }

  tree(at = this.marker) {
    if(at instanceof Feature) at = this.features.indexOf(at) + 1
    // Return last state if current feature hasn't been executed yet
    return this.cache[at]// || this.cache[this.marker - 1]
  }

  moveMarkerToFeature(feature) {
    this.marker = this.features.indexOf(feature) + 1
  }

  isCurrentFeature(feature) {
    return this.marker == this.features.indexOf(feature) + 1
  }

  previousFeature(feature) {
    let i = this.features.indexOf(feature)
    return this.features[i - 1]
  }

  insertFeature(feature) {
    this.features.splice(this.marker, 0, feature)
    this.last_change_index = Math.min(this.last_change_index, this.marker)
    // console.log('insertFeature', this.last_change_index, this.marker)
    this.marker++
    this.evaluate()
  }

  invalidateFeature(feature) {
    this.last_change_index = Math.min(this.features.indexOf(feature), this.last_change_index)
    // console.log('invalidating -> last_change_index', this.last_change_index)
  }

  // repairFeature(feature) {
  //   const index = this.features.indexOf(feature)
  //   const comp = this.cache[index]
  //   feature.repair(comp)
  //   this.invalidateFeature(feature)
  // }

  removeFeature(feature) {
    // this.removal_modifications.append(
    //   &mut feature.borrow().feature_type.as_feature().modified_components()
    // );
    let index = this.features.indexOf(feature)
    this.features = this.features.filter(f => f != feature )
    if(this.marker > index) {
      this.marker -= 1
      this.last_eval_index -= 1
    }
    // this.last_change_index = Math.max(0, Math.min(this.last_change_index, index))
    this.last_change_index = Math.min(this.last_change_index, index)
    // console.log('removeFeature', this.last_change_index, this.features)
  }

  evaluate() {
    const last_change = this.last_change_index
    console.log('evaluate', last_change, this.marker)
    this.regenerate(Math.min(last_change, this.marker), this.marker)
    const [from, to] = ordered(this.last_eval_index, this.marker)
    this.last_eval_index = this.marker
    return this.componentsModified(Math.min(from, last_change), to)
  }

  regenerate(from, to) {
    // this.cache.resize(this.features.len() + 1, Component::default());
    // console.log('before regenerate', from, to, this.cache)
    for(let i = from; i < to; i++) {
      const feature = this.features[i]
      let newComp = this.cache[i].deepClone()
      let j = i + 1
      // console.log('updating cache at with', j, newComp)
      this.cache[j] = newComp
      feature.execute(newComp)
      if(feature.error && feature.error.type == 'error') {
        this.cache[j] = this.cache[i].deepClone()
      } else {
        // let repair_error = feature.modified_components()
        //   .find_map(|id| newComp.find_child_mut(id).unwrap().compound.repair().err() )
        //   .map(|error| FeatureError::Error(error) );
        // if repair_error.is_some() {
        //   feature.error = repair_error;
        //   this.cache[j] = this.cache[i].deep_clone()
        // } else {
          // console.log('updating cache at with', j, newComp)
          // this.cache[j] = newComp
        // }
      }
      this.last_change_index = j
    }
    // console.log('after regenerate', from, to, this.cache)
  }

  reorder(feature, other, after) {
    this.features = this.features.filter(f => f != feature )
    const otherIndex = this.features.indexOf(other)
    const targetIndex = otherIndex + (after ? 1 : 0)
    this.features.splice(targetIndex, 0, feature)
    this.invalidateFeature(feature)
    this.invalidateFeature(other)
  }

  componentsModified(from, to) {
    // Find unique ids of modified components in given range
    // for(let i = from; i < to; i++) {
    //   const feature = this.features[i]
    // }
    let compIds = [...new Set(
      arrayRange(from, to - 1)
        .map(i => this.features[i].modifiedComponents() )
        // comp_ids.append(&mut this.removal_modifications);
        .flat()
    )]
    // Filter children whose parents are already part of the set
    const comps = compIds.map(id => this.cache[to].findChild(id) )
    // console.log(compIds, this.cache, comps)
    compIds = compIds.filter(id => !comps.some(comp => hasChild(comp, id) ) )
    return compIds
  }

  getFutureChildIds(compId) {
    const tree = this.finalTree()
    // const comp = this.getFutureComp(compId, tree)
    const comp = tree.findChild(compId)
    // return this.getChildIds(comp)
    return comp.getChildIds()
  }

  // getFutureComp(id, tree) {
  //   if(id == tree.id) return tree
  //   for(const child of tree.children) {
  //     const self = this.getFutureComp(id, child)
  //     if(self) return self
  //   }
  // }

  // getChildIds(comp) {
  //   let ids = [comp.id]
  //   for(const child of comp.children) {
  //     ids = ids.concat(this.getChildIds(child))
  //   }
  //   return ids
  // }

  finalTree() {
    let tree = new Component(null, 'id-0')
    tree.creator = this.baseCompDef
    this.features.forEach(feature => {
      if(feature instanceof CreateComponentFeature) {
        let parent = tree.findChild(feature.parent)
        const child = new Component(parent, feature.id)
        child.creator = feature
        parent.children.push(child)
      }
    })
    return tree
  }

  makeColor() {
    const existingColors =
      this.features
      .filter(feature => feature instanceof CreateComponentFeature )
      .map(feature => feature.definition.color )
      .concat([this.baseCompDef.color])
    return makeColor(existingColors)
  }

  dump() {
    return {
      marker: this.marker,
      features: this.features,
      baseCompDef: this.baseCompDef,
    }
  }

  static undump(dump) {
    const out = new Timeline(dump.baseCompDef)
    Object.assign(out, dump)
    return out
  }
}
Serialize.register(Timeline, 'Timeline')


function ordered(a, b) {
  return a < b ? [a, b] : [b, a]
}

function hasChild(comp, id) {
  const child = comp.findChild(id)
  return child && !child == comp
}
