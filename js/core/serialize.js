import * as THREE from 'three'

export default class Serialize {
  static classes = {}

  static register(klass, className) {
    this.classes[className] = klass
  }

  static serialize(obj) {
    // Arrays
    if(Array.isArray(obj)) return obj.map(value => this.serialize(value) )

    // Base Types
    if(!isObject(obj)) return obj

    let dump
    if(obj.constructor.undump) {
      // Custom Classes
      dump = {
        $class: Object.keys(this.classes).find(cn => this.classes[cn] === obj.constructor ),
        ...(obj.dump ? obj.dump() : obj),
      }
      if(!dump.$class) throw obj.constructor.name + " has not been registered with Serialize"

    } else {
      // Generic Objects & unregistered custom classes
      dump = obj
      if(obj.constructor !== Object) console.warn("Losing type information about class " + obj.constructor.name)
    }

    Object.keys(dump).forEach(key => {
      dump[key] = this.serialize(dump[key])
    })

    return dump
  }

  static deserialize(dump, context) {
    // Arrays
    if(Array.isArray(dump)) return dump.map(value => this.deserialize(value, context) )

    // Base Types
    if(!isObject(dump)) return dump

    // Generic Objects & Custom Classes
    Object.keys(dump).forEach(key => {
      dump[key] = this.deserialize(dump[key], context)
    })

    if(dump.$class) {
      // Custom Classes
      const klass = this.classes[dump.$class]
      return klass.undump(dump, context)

    } else {
      // Generic Objects
      return dump
    }
  }

  static stringify(obj) {
    return JSON.stringify(this.serialize(obj), null, 2)
  }

  static parse(str, context) {
    return this.deserialize(JSON.parse(str), context)
  }
}

function isObject(value) {
  return typeof value === 'object' && value !== null
}


THREE.Vector3.prototype.dump = function() {
  return { elements: this.toArray() }
}

THREE.Vector3.undump = function(dump) {
  return new THREE.Vector3().fromArray(dump.elements)
}

Serialize.register(THREE.Vector3, 'Vector3')


THREE.Matrix4.prototype.dump = function() {
  return { elements: this.toArray() }
}

THREE.Matrix4.undump = function(dump) {
  return new THREE.Matrix4().fromArray(dump.elements)
}

Serialize.register(THREE.Matrix4, 'Matrix4')
