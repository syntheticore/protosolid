import Serialize from './serialize.js'
import { Component, ComponentDefinition, syncComponentInstances } from './component.js'
import { ConvertBodyToComponentFeature, CreateComponentFeature, CreateComponentInstanceFeature, PatternFeature, Feature } from './features.js'
import { CentroidSolver } from './centroid.js'
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
    this.change_version = 0
    this.centroids = new CentroidSolver()
    this.centroids.sync(this.cache)
    // removal_modifications: Vec<CompRef>,
  }

  tree(at = this.marker) {
    if(at instanceof Feature) at = this.features.indexOf(at) + 1
    // Return last state if current feature hasn't been executed yet
    return this.cache[Math.min(at, this.cache.length - 1)]
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

  insertFeature(feature, evaluate = true) {
    // A removed or canceled feature may have left a result cached directly
    // after the marker. Never expose that stale result for the new feature
    // while its evaluation is pending.
    this.cache.length = Math.min(this.cache.length, this.marker + 1)
    this.features.splice(this.marker, 0, feature)
    this.last_change_index = Math.min(this.last_change_index, this.marker)
    this.change_version++
    // console.log('insertFeature', this.last_change_index, this.marker)
    this.marker++
    if(evaluate) this.evaluate()
  }

  invalidateFeature(feature) {
    this.last_change_index = Math.min(this.features.indexOf(feature), this.last_change_index)
    this.change_version++
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
    // Rebuilding the array through a reactive timeline would compare and
    // store proxies. Splicing keeps the raw features that lookups expect.
    this.features.splice(index, 1)
    if(this.marker > index) {
      this.marker -= 1
      this.last_eval_index -= 1
    }
    // this.last_change_index = Math.max(0, Math.min(this.last_change_index, index))
    this.last_change_index = Math.min(this.last_change_index, index)
    this.change_version++
    // console.log('removeFeature', this.last_change_index, this.features)
  }

  // Replace all features from index on. Like edits, their outdated results
  // stay visible until evaluation replaces them.
  replaceFeatures(index, features) {
    this.features.splice(index, Infinity, ...features)
    this.last_change_index = Math.min(this.last_change_index, index)
    this.last_eval_index = Math.min(this.last_eval_index, index)
    this.change_version++
  }

  evaluate() {
    let last_change = Math.min(this.last_change_index, this.marker)
    console.log('evaluate', last_change, this.marker)
    this.regenerate(last_change, this.marker)
    for(let iteration = 0; ; iteration++) {
      const index = this.updateCentroids(iteration)
      if(index === null) break
      last_change = Math.min(last_change, index)
      this.regenerate(index, this.marker)
    }
    const [from, to] = ordered(this.last_eval_index, this.marker)
    this.last_eval_index = this.marker
    return this.componentsModified(Math.min(from, last_change), to)
  }

  async evaluateAsync(onFeature) {
    // Let pointer events settle for this frame before capturing the pass.
    // Capturing first would let every drag event cancel it before execution.
    await yieldToBrowser()
    let lastChange = Math.min(this.last_change_index, this.marker)
    const marker = this.marker
    if(!await this.regenerateAsync(lastChange, marker, onFeature)) return null
    for(let iteration = 0; ; iteration++) {
      const index = this.updateCentroids(iteration)
      if(index === null) break
      lastChange = Math.min(lastChange, index)
      if(!await this.regenerateAsync(index, marker, onFeature)) return null
    }
    const [from, to] = ordered(this.last_eval_index, marker)
    this.last_eval_index = marker
    return this.componentsModified(Math.min(from, lastChange), to)
  }

  regenerate(from, to) {
    for(let i = from; i < to; i++) {
      this.regenerateFeature(i)
    }
  }

  async regenerateAsync(from, to, onFeature) {
    const version = this.change_version
    for(let i = from; i < to; i++) {
      onFeature(this.features[i], i, to)
      if(i > from) await yieldToBrowser()

      // A newer edit or structural change supersedes this pass. In
      // particular, do not republish a feature that was canceled while this
      // task was yielding.
      if(this.change_version != version) return false

      this.regenerateFeature(i)
    }
    return true
  }

  regenerateFeature(index) {
    const feature = this.features[index]
    const tree = this.cache[index].deepClone()
    this.cache[index + 1] = tree
    this.centroids.execute(feature, tree, () => {
      feature.execute(tree)
      syncComponentInstances(tree)
    })
    if(feature.error?.type == 'error') this.cache[index + 1] = this.cache[index].deepClone()
    this.last_change_index = index + 1
  }

  updateCentroids(iteration) {
    const finalTree = this.marker == this.features.length && this.cache[this.marker]
    return this.centroids.update(this.cache, finalTree, this.features, iteration)
  }

  reorder(feature, other, after) {
    const sourceIndex = this.features.indexOf(feature)
    this.features.splice(sourceIndex, 1)
    const otherIndex = this.features.indexOf(other)
    const targetIndex = otherIndex + (after ? 1 : 0)
    this.features.splice(targetIndex, 0, feature)
    this.last_change_index = Math.min(this.last_change_index, sourceIndex, targetIndex)
    this.change_version++
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
        child.creator = feature.definition
        parent.children.push(child)
      } else if(feature instanceof ConvertBodyToComponentFeature) {
        const parent = tree.findChild(feature.componentId)
        if(!parent) return
        const child = new Component(parent, feature.id)
        child.creator = feature.definition
        parent.children.push(child)
      } else if(feature instanceof CreateComponentInstanceFeature) {
        const source = tree.findChild(feature.sourceId)
        const parent = tree.findChild(feature.parentId)
        if(source && parent) createStructuralInstance(parent, source, feature.id)
      } else if(feature instanceof PatternFeature && feature.inputs) {
        const parent = tree.findChild(feature.componentId)
        const componentRefs = feature.inputs()
          .filter(reference => reference.componentReference)
        componentRefs.forEach(reference => {
          const source = tree.findChild(reference.componentReference.componentId)
          if(!source || !parent) return
          for(const key of feature.instanceKeys()) {
            createStructuralInstance(parent, source, `${feature.id}/instance/${key}/${source.id}`)
          }
        })
      } else if(feature.addStructuralComponents) {
        feature.addStructuralComponents(tree)
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

function createStructuralInstance(parent, source, id) {
  const child = new Component(parent, id)
  child.creator = source.creator
  child.instanceOf = source.sourceId()
  child.sourceOccurrence = source.id
  parent.children.push(child)
  source.children.forEach(sourceChild =>
    createStructuralInstance(child, sourceChild, `${id}/${sourceChild.id}`)
  )
}
Serialize.register(Timeline, 'Timeline')


function ordered(a, b) {
  return a < b ? [a, b] : [b, a]
}

function hasChild(comp, id) {
  const child = comp.findChild(id)
  return child && !child == comp
}

function yieldToBrowser() {
  return new Promise(resolve => {
    requestAnimationFrame(() => setTimeout(resolve, 0))
  })
}
