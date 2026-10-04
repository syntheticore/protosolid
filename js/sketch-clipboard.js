import * as THREE from 'three'
import Serialize from './core/serialize.js'
import { makeID } from './core/id.js'
import { SketchElement } from './core/geom2d.js'
import { ElemRef, ProfileOffsetConstraint } from './core/sketch.js'

let clipboard = null

export function copySketchSelection(sketch, selection, state = sketch.dump()) {
  const elements = new Set(selection.map(item => item instanceof ElemRef ? item.curve() : item)
    .filter(item => item instanceof SketchElement && state.elements.includes(item)))
  if(!elements.size) return false

  // Derived offsets need their defining curves to survive sketch history and saving.
  let expanded
  do {
    expanded = false
    state.constraints.filter(constraint => constraint instanceof ProfileOffsetConstraint)
      .forEach(constraint => {
        if(!constraint.outputs().some(curve => elements.has(curve))) return
        constraint.items.forEach(item => {
          const curve = item.curve()
          if(!state.elements.includes(curve) || elements.has(curve)) return
          elements.add(curve)
          expanded = true
        })
      })
  } while(expanded)

  const curves = [...elements]
  const constraints = state.constraints.filter(constraint =>
    constraint.items.every(item => elements.has(item.curve())))
  clipboard = JSON.stringify({
    elements: Serialize.serialize(curves),
    constraints: constraints.map(constraint => ({
      ...Serialize.serialize(constraint),
      items: constraint.items.map(item => ({ curve: curves.indexOf(item.curve()), index: item.index })),
    })),
  })
  return true
}

export function hasSketchClipboard() { return clipboard !== null }

export function pasteSketchSelection(sketch) {
  const { elements, constraints } = Serialize.parse(clipboard)
  elements.forEach(elem => {
    elem.id = makeID()
    sketch.add(elem)
  })
  constraints.forEach(constraint => {
    constraint.id = makeID()
    constraint.sketch = sketch
    constraint.items = constraint.items.map(item => new ElemRef(elements[item.curve], item.index))
  })
  constraints.filter(constraint => constraint instanceof ProfileOffsetConstraint)
    .forEach(constraint => constraint.rebuild())
  const bounds = new THREE.Box3().setFromPoints(elements.flatMap(elem => elem.handles()))
  return { elements, constraints, anchor: bounds.getCenter(new THREE.Vector3()) }
}
