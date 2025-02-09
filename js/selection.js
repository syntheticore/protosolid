
export class Selection {
  constructor(items) {
    this.items = items || []
    this.set = new Set(items)
  }

  handle(item, ctrlPressed) {
    if(ctrlPressed) {
      this.toggle(item)
    } else {
      this.only(item)
    }
  }

  toggle(item) {
    if(this.has(item)) {
      this.delete(item)
    } else {
      this.add(item)
    }
  }

  add(item) {
    if(!this.has(item)) this.items.push(item)
    this.set.add(item)
  }

  delete(item) {
    this.items = this.items.filter(i => i != item )
    this.set.delete(item)
  }

  has(item) {
    this.set.has(item)
  }

  only(item) {
    this.items = [item]
    this.set = new Set([item])
  }

  clear() {
    this.items = []
    this.set = new Set()
  }
}
