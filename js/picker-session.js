// Owns one editing panel's picker lifecycle. A click completes a normal pick;
// a modifier hold completes on release, without advancing to another picker.
export default class PickerSession {
  constructor(bus, callbacks) {
    this.bus = bus
    this.callbacks = callbacks
    this.active = null
    this.lastKey = null
    this.revision = 0
    this.onPicked = item => {
      const session = this.active
      if(!session) return
      callbacks.select(session.key, item)
      session.changed = true
      if(!bus.isCtrlPressed) this.finish(true)
    }
    this.onModifier = pressed => {
      if(pressed) {
        if(!this.active && this.lastKey && callbacks.available(this.lastKey)) {
          this.start(this.lastKey)
        }
      } else if(this.active) {
        this.finish(false)
      }
    }
    bus.on('picked', this.onPicked)
    bus.on('ctrl-pressed', this.onModifier)
  }

  start(key) {
    this.cancel()
    if(this.disposed || !this.callbacks.available(key)) return Promise.resolve(false)
    this.lastKey = key
    return new Promise(resolve => {
      this.active = { key, changed: false, resolve }
      this.callbacks.activate(key)
    })
  }

  detach() {
    const session = this.active
    this.active = null
    if(session) this.callbacks.deactivate()
    return session
  }

  async finish(advance) {
    const revision = ++this.revision
    const session = this.detach()
    if(!session) return
    const current = () => this.revision === revision
    let completed = false
    try {
      await this.callbacks.complete?.(session.key, session.changed, current)
      completed = advance && current()
    } finally {
      // Cancellation and failed updates must also settle callers awaiting a pick.
      session.resolve(completed)
    }
  }

  cancel() {
    this.revision++
    this.detach()?.resolve(false)
  }

  dispose() {
    this.disposed = true
    this.cancel()
    this.bus.off('picked', this.onPicked)
    this.bus.off('ctrl-pressed', this.onModifier)
  }
}
