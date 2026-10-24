import { markRaw } from 'vue'

import { describeConflict, describeFeatureChanges } from './merge-conflicts.js'

const resolvers = {
  lastId: (base, ours, theirs) => bigIntMax(...[ours, theirs].map(id => BigInt(id.slice(0, -1)) )).toString() + 'n',
  'timeline.marker': (base, ours) => ours,
}

// Merge of a branch into the current one. While the session is active, the
// document shows the merged result and conflicts can be resolved one by one,
// until the merge is committed or canceled.
// Call its methods through document.activeMerge, so changes stay reactive.
export class MergeSession {
  constructor(document, branch) {
    this.document = document
    this.repository = document.repository
    this.branch = branch
    this.current = this.repository.currentBranch
    this.resolutions = {}
    this.plan = null
    this.changes = []
    this.setPlan(this.planMerge())
    this.conflicts = this.plan.conflicts.map(conflict => ({ ...conflict, ...describeConflict(conflict, document.timeline) }) )
    this.conflicts.forEach(conflict => this.resolutions[conflict.key] = 'ours' )
    // Left empty, the commit uses the default message
    this.message = ''
    this.defaultMessage = `Merge branch '${branch}' into ${this.current}`
    // The version shown before merging, restored on cancel
    this.original = null
    // The evaluated merge result, once it is shown
    this.preview = null
  }

  async start() {
    const { timeline, materials, activeComponent } = this.document
    this.original = markRaw({ timeline, materials, activeComponent })
    await this.update()
  }

  planMerge() {
    return this.repository.planMerge(this.branch, { resolutions: this.resolutions, resolvers })
  }

  setPlan(plan) {
    if(plan.type == 'merge') {
      // Show every merged feature
      plan.content.timeline.marker = plan.content.timeline.features.length
    }
    this.plan = markRaw(plan)
    if(plan.type != 'up-to-date') this.changes = describeFeatureChanges(this.repository.read(), this.mergedContent(), plan.conflicts)
  }

  mergedContent() {
    return this.plan.type == 'merge' ? this.plan.content : this.repository.read(this.plan.commitId)
  }

  async resolve(keys, side) {
    keys.forEach(key => this.resolutions[key] = side )
    this.setPlan(this.planMerge())
    await this.update()
  }

  // Evaluate and show the merged version of the current plan
  async update() {
    const document = this.document
    const plan = this.plan
    const content = this.mergedContent()
    content.timeline.marker = content.timeline.features.length
    // Deserialization works in place, so keep plan content intact for the commit
    const { dump, materials } = document.readVersion(structuredClone(content))
    if(!dump) return
    await document.calculateDump(dump, 'Merging', this.branch)
    // The merge may have been committed, canceled or replanned meanwhile
    if(document.activeMerge !== this || this.plan !== plan) return
    this.preview = markRaw({ plan, views: dump.views })
    document.showVersion({ timeline: dump.timeline, materials })
  }

  async commit() {
    const document = this.document
    if(this.preview?.plan !== this.plan) return
    this.repository.commitMerge(this.plan, this.message.trim() || this.defaultMessage)
    if(Array.isArray(this.preview.views)) document.views = this.preview.views
    document.activeMerge = null
    await document.writeRepository()
  }

  cancel() {
    this.document.showVersion(this.original)
    this.document.activeMerge = null
  }
}

const bigIntMax = (...args) => args.reduce((m, e) => e > m ? e : m)
