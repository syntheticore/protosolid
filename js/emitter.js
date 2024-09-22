export default class Emitter {
  constructor() {
    this.listeners = {}
  }

  on(name, handler) {
    this.listeners[name] ||= []
    this.listeners[name].push(handler)
  }

  emit(name, ...args) {
    if(this.listeners[name]) this.listeners[name].forEach(handler => handler(...args) )
  }

  off(name, handler) {
    const handlers = this.listeners[name]
    if(!handlers) return
    if(handler) {
      const index = handlers.indexOf(handler)
      if(index == -1) return
      handlers.splice(index, 1)
    } else {
      delete this.listeners[name]
    }
  }

  once(name, handler) {
    const fn = (...args) => {
      this.off(name, fn)
      handler(...args)
    }
    this.on(name, fn)
  }
}
