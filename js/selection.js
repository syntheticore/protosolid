
export class Selection {
  constructor(items) {
    this.set = new Set(items)
  }

  toggle(item) {
    if(this.has(item)) {
      return this.delete(item)
    } else {
      return this.add(item)
    }
  }

  add(item) {
    const selection = new Selection(this.set)
    selection.set.add(item)
    return selection
  }

  delete(item) {
    const selection = new Selection(this.set)
    selection.set.delete(item)
    return selection
  }

  has(item) {
    return this.set.has(item)
  }

  only(item) {
    return new Selection([item])
  }

  clear() {
    return new Selection()
  }

  handle(item, ctrlPressed) {
    if(ctrlPressed) {
      return this.toggle(item)
    } else {
      return this.only(item)
    }
  }
}
