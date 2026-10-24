// A miniature, single-file version control store for ProtoSolid documents.
//
// Every save becomes a commit pointing at a root object. Document content is
// split into content-addressed objects (a Merkle tree of the JSON dump), so
// unchanged features, sketches and long strings are shared between commits
// and each new version only stores the parts that actually changed.
//
// File layout:
// {
//   format: 'protosolid-history', version: 1,
//   head: { branch: 'master' } | { commit: '<id>' },
//   branches: { master: '<commit id>' },
//   lastId: '<highest document id ever issued>',
//   commits: { '<id>': { parents, message, date, root } },
//   objects: { '<id>': <JSON value with { $obj: '<id>' } placeholders> },
// }

export const REPOSITORY_FORMAT = 'protosolid-history'

// Nodes whose compact JSON is at least this long are stored as separate objects
const CHUNK_SIZE = 96

export function isRepositoryDump(dump) {
  return dump?.format === REPOSITORY_FORMAT
}

export class MergeError extends Error {}

export class Repository {
  constructor(dump = {}) {
    this.head = dump.head || { branch: 'master' }
    this.branches = dump.branches || {}
    this.commits = dump.commits || {}
    this.objects = dump.objects || {}
    this.lastId = dump.lastId || '0'
  }

  get currentBranch() {
    return this.head.branch || null
  }

  get headId() {
    return this.head.branch ? (this.branches[this.head.branch] || null) : this.head.commit
  }

  get isEmpty() {
    return !this.headId
  }

  // Stored commits and objects are immutable; only their lookup tables and refs change.
  clone() {
    return new Repository({
      head: { ...this.head },
      branches: { ...this.branches },
      commits: { ...this.commits },
      objects: { ...this.objects },
      lastId: this.lastId,
    })
  }

  // Content

  read(commitId = this.headId) {
    return this.materialize({ $obj: this.commits[commitId].root })
  }

  materialize(node) {
    if(isRef(node)) return this.materialize(this.objects[node.$obj])
    if(Array.isArray(node)) return node.map(value => this.materialize(value) )
    if(isObject(node)) {
      const out = {}
      for(const key in node) out[key] = this.materialize(node[key])
      return out
    }
    return node
  }

  store(value) {
    let node = value
    if(Array.isArray(value)) {
      node = value.map(item => this.store(item) )
    } else if(isObject(value)) {
      node = {}
      for(const key in value) node[key] = this.store(value[key])
    } else if(typeof value != 'string') {
      return value
    }
    const json = JSON.stringify(node)
    if(json.length < CHUNK_SIZE) return node
    const id = hash(json)
    this.objects[id] = node
    return { $obj: id }
  }

  storeRoot(content) {
    const node = this.store(content)
    if(isRef(node)) return node.$obj
    const id = hash(JSON.stringify(node))
    this.objects[id] = node
    return id
  }

  // Commits & refs

  commit(content, message, parents = [this.headId].filter(Boolean)) {
    const commit = {
      parents,
      message,
      date: new Date().toISOString(),
      root: this.storeRoot(content),
    }
    const id = hash(JSON.stringify(commit))
    this.commits[id] = commit
    this.moveHead(id)
    return id
  }

  moveHead(commitId) {
    if(this.head.branch) {
      this.branches[this.head.branch] = commitId
    } else {
      this.head = { commit: commitId }
    }
  }

  checkout(target) {
    if(this.branches[target]) {
      this.head = { branch: target }
    } else {
      // Prefer staying attached when the commit is a branch tip
      const branch = Object.keys(this.branches).find(name => this.branches[name] == target )
      this.head = branch ? { branch } : { commit: target }
    }
  }

  createBranch(name, commitId = this.headId) {
    if(this.branches[name]) throw new Error(`A branch named '${name}' already exists.`)
    this.branches[name] = commitId
  }

  deleteBranch(name) {
    if(this.head.branch == name) throw new Error('The current branch cannot be deleted.')
    delete this.branches[name]
  }

  renameBranch(name, newName) {
    if(this.branches[newName]) throw new Error(`A branch named '${newName}' already exists.`)
    this.branches[newName] = this.branches[name]
    delete this.branches[name]
    if(this.head.branch == name) this.head = { branch: newName }
  }

  isAncestor(ancestorId, commitId) {
    return this.ancestors(commitId).has(ancestorId)
  }

  ancestors(commitId) {
    const seen = new Set()
    const queue = [commitId]
    while(queue.length) {
      const id = queue.pop()
      if(seen.has(id)) continue
      seen.add(id)
      queue.push(...this.commits[id].parents)
    }
    return seen
  }

  mergeBase(a, b) {
    const ancestorsA = this.ancestors(a)
    const common = [...this.ancestors(b)].filter(id => ancestorsA.has(id) )
    // The best common ancestor is the one not reachable from any other candidate
    return common.find(id => !common.some(other => other != id && this.isAncestor(id, other) )) || null
  }

  // Three-way merge of the given branch into HEAD.
  // Returns { type: 'up-to-date' | 'fast-forward' | 'merge', content, conflicts }.
  // Resolutions pick 'ours' or 'theirs' per conflict key, unresolved conflicts keep ours.
  planMerge(branch, { resolutions = {}, resolvers = {} } = {}) {
    const ours = this.headId
    const theirs = this.branches[branch]
    if(!theirs) throw new MergeError(`Unknown branch '${branch}'.`)
    if(this.isAncestor(theirs, ours)) return { type: 'up-to-date', conflicts: [] }
    if(this.isAncestor(ours, theirs)) return { type: 'fast-forward', commitId: theirs, conflicts: [] }

    const base = this.mergeBase(ours, theirs)
    const conflicts = []
    const merger = new TreeMerger(this, resolutions, resolvers, conflicts)
    const root = id => id && ({ $obj: this.commits[id].root })
    const merged = merger.merge(root(base), root(ours), root(theirs), [])
    return { type: 'merge', theirs, content: this.materialize(merged), conflicts }
  }

  commitMerge(plan, message) {
    if(plan.type == 'fast-forward') {
      this.moveHead(plan.commitId)
      return plan.commitId
    }
    return this.commit(plan.content, message, [this.headId, plan.theirs])
  }

  // Commits ordered newest first, children always before their parents
  log() {
    const placed = new Set()
    const out = []
    // Parents are always placed before their children, otherwise by date
    const place = id => {
      if(placed.has(id)) return
      placed.add(id)
      this.commits[id].parents.forEach(place)
      out.push({ id, ...this.commits[id] })
    }
    Object.keys(this.commits)
      .sort((a, b) => this.commits[a].date.localeCompare(this.commits[b].date) )
      .forEach(place)
    return out.reverse()
  }

  // Serialization

  toJSON() {
    this.collectGarbage()
    return {
      format: REPOSITORY_FORMAT,
      version: 1,
      head: this.head,
      branches: this.branches,
      lastId: this.lastId,
      commits: this.commits,
      objects: this.objects,
    }
  }

  stringify() {
    return JSON.stringify(this)
  }

  // Drop commits unreachable from any branch or HEAD, and objects unreachable from those
  collectGarbage() {
    const reachable = new Set()
    const tips = [...Object.values(this.branches), this.headId].filter(Boolean)
    tips.forEach(tip => this.ancestors(tip).forEach(id => reachable.add(id) ))
    const commits = {}
    reachable.forEach(id => commits[id] = this.commits[id] )
    this.commits = commits

    const used = new Set()
    const mark = node => {
      if(isRef(node)) {
        if(used.has(node.$obj)) return
        used.add(node.$obj)
        mark(this.objects[node.$obj])
      } else if(Array.isArray(node)) {
        node.forEach(mark)
      } else if(isObject(node)) {
        Object.values(node).forEach(mark)
      }
    }
    Object.values(this.commits).forEach(commit => mark({ $obj: commit.root }) )
    const objects = {}
    used.forEach(id => objects[id] = this.objects[id] )
    this.objects = objects
  }
}


class TreeMerger {
  constructor(repository, resolutions, resolvers, conflicts) {
    this.repository = repository
    this.resolutions = resolutions
    this.resolvers = resolvers
    this.conflicts = conflicts
  }

  expand(node) {
    return isRef(node) ? this.expand(this.repository.objects[node.$obj]) : node
  }

  merge(base, ours, theirs, path) {
    if(same(ours, theirs)) return ours
    const resolver = this.resolvers[pathKey(path)]
    if(resolver) return resolver(...[base, ours, theirs].map(node => this.repository.materialize(node) ))
    if(same(base, ours)) return theirs
    if(same(base, theirs)) return ours

    const b = this.expand(base)
    const o = this.expand(ours)
    const t = this.expand(theirs)

    if(Array.isArray(o) && Array.isArray(t) && hasIds(o.map(node => this.expand(node) )) && hasIds(t.map(node => this.expand(node) ))) {
      return this.mergeIdentified(Array.isArray(b) ? b : [], o, t, path)
    }

    if(isPlainObject(o) && isPlainObject(t) && o.$class == t.$class) {
      const baseObject = isPlainObject(b) ? b : {}
      const out = {}
      const keys = new Set([...Object.keys(o), ...Object.keys(t)])
      keys.forEach(key => {
        const value = this.merge(baseObject[key], o[key], t[key], [...path, key])
        if(value !== undefined) out[key] = value
      })
      return out
    }

    return this.conflict(path, ours, theirs)
  }

  // Merge arrays of objects carrying an id (features, sketch elements, views...)
  mergeIdentified(base, ours, theirs, path) {
    const byId = list => new Map(list.map(node => [this.expand(node).id, node]) )
    const baseMap = byId(base)
    const oursMap = byId(ours)
    const theirsMap = byId(theirs)
    const merged = new Map()

    ours.forEach(node => {
      const id = this.expand(node).id
      const itemPath = [...path, { id }]
      if(theirsMap.has(id)) {
        merged.set(id, this.merge(baseMap.get(id), node, theirsMap.get(id), itemPath))
      } else if(baseMap.has(id)) {
        // Deleted in theirs: drop unless we modified it
        if(!same(baseMap.get(id), node) && this.conflict(itemPath, node, undefined) !== undefined) merged.set(id, node)
      } else {
        merged.set(id, node)
      }
    })

    theirs.forEach(node => {
      const id = this.expand(node).id
      if(oursMap.has(id)) return
      const itemPath = [...path, { id }]
      if(baseMap.has(id)) {
        // Deleted in ours: keep deletion unless they modified it
        if(same(baseMap.get(id), node) || this.conflict(itemPath, undefined, node) === undefined) return
      }
      merged.set(id, node)
    })

    // Compare the order of surviving base entries separately from their values.
    const ids = list => list.map(node => this.expand(node).id)
    const shared = new Set([...baseMap.keys()].filter(id => oursMap.has(id) && theirsMap.has(id)))
    const baseOrder = ids(base).filter(id => shared.has(id))
    const ourOrder = ids(ours).filter(id => shared.has(id))
    const theirOrder = ids(theirs).filter(id => shared.has(id))
    let primary = ours
    if(!same(ourOrder, theirOrder)) {
      if(same(baseOrder, ourOrder)) primary = theirs
      else if(!same(baseOrder, theirOrder)) primary = this.conflict([...path, '$order'], ours, theirs, [ourOrder, theirOrder])
    }

    const secondary = ids(primary === ours ? theirs : ours)
    const order = ids(primary).filter(id => merged.has(id))
    secondary.forEach((id, index) => {
      if(!merged.has(id) || order.includes(id)) return
      // Keep additions from the other branch beside their closest surviving anchor.
      let position = -1
      for(let i = index - 1; i >= 0; i--) {
        const previous = order.indexOf(secondary[i])
        if(previous >= 0) {
          position = previous + 1
          break
        }
      }
      if(position < 0) {
        const next = secondary.slice(index + 1).find(nextId => order.includes(nextId))
        position = next === undefined ? order.length : order.indexOf(next)
      }
      order.splice(position, 0, id)
    })

    return order.map(id => merged.get(id))
  }

  // Path segments are object keys, or { id } for entries of identified lists.
  // Shown values are what each branch has at that path, undefined when deleted.
  conflict(path, ours, theirs, shown = [ours, theirs]) {
    const [oursValue, theirsValue] = shown.map(node => this.repository.materialize(node) )
    const key = pathKey(path)
    this.conflicts.push({ key, path, ours: oursValue, theirs: theirsValue })
    return this.resolutions[key] == 'theirs' ? theirs : ours
  }
}

function pathKey(path) {
  return path.map(segment => typeof segment == 'string' ? segment : `[${segment.id}]` ).join('.')
}


function isObject(value) {
  return typeof value === 'object' && value !== null
}

function isPlainObject(value) {
  return isObject(value) && !Array.isArray(value)
}

function isRef(node) {
  return isPlainObject(node) && typeof node.$obj == 'string'
}

function hasIds(list) {
  const ids = list.map(item => isPlainObject(item) ? item.id : undefined )
  return ids.every(id => id !== undefined && id !== null ) && new Set(ids).size == ids.length
}

// Stored nodes are canonical, so equal content always produces equal JSON
function same(a, b) {
  return JSON.stringify(a) === JSON.stringify(b)
}

// 128 bit non-cryptographic string hash (cyrb128), rendered as 32 hex digits
function hash(str) {
  let h1 = 1779033703, h2 = 3144134277, h3 = 1013904242, h4 = 2773480762
  for(let i = 0, k; i < str.length; i++) {
    k = str.charCodeAt(i)
    h1 = h2 ^ Math.imul(h1 ^ k, 597399067)
    h2 = h3 ^ Math.imul(h2 ^ k, 2869860233)
    h3 = h4 ^ Math.imul(h3 ^ k, 951274213)
    h4 = h1 ^ Math.imul(h4 ^ k, 2716044179)
  }
  h1 = Math.imul(h3 ^ (h1 >>> 18), 597399067)
  h2 = Math.imul(h4 ^ (h2 >>> 22), 2869860233)
  h3 = Math.imul(h1 ^ (h3 >>> 17), 951274213)
  h4 = Math.imul(h2 ^ (h4 >>> 19), 2716044179)
  h1 ^= (h2 ^ h3 ^ h4)
  h2 ^= h1
  h3 ^= h1
  h4 ^= h1
  return [h1, h2, h3, h4].map(h => (h >>> 0).toString(16).padStart(8, '0') ).join('')
}
