import * as THREE from 'three'

import { Aluminum, DarkViolet, Polystyrene, CarPaint, Glass } from '../material.js'
import { Canvas } from '../core/component.js'
import { ConstructionHelper } from '../core/helpers.js'
import { loadFile } from '../utils.js'
import { ToolError, itemId, resolveItem, findSketch, findFeature, sketchComponent, describeMaterial, describeVariant, describeStored } from './describe.js'
import { editable, closeEditing, settle, editableComponent, findStored, storedKinds, option, number, bool, assign } from './common.js'

// Component properties outside the timeline: materials, names, colors,
// configurations and reference images

const presets = {
  Aluminum,
  'Aluminum Anodized': DarkViolet,
  Polystyrene,
  'Car Paint': CarPaint,
  Glass,
}

const color = (value, key) => {
  if(!/^#[0-9a-f]{6}$/i.test(value)) throw new ToolError(`${key} must be a hex color like #3080ff`)
  return value
}
const text = (value, key) => {
  if(typeof value != 'string' || !value.trim()) throw new ToolError(`${key} must be a non-empty string`)
  return value.trim()
}
const range = (min, max) => (value, key) => number(value, key, { min, max })

const materialProperties = {
  title: text,
  density: range(0.001, Infinity),
  elasticModulus: range(0, Infinity),
  poissonRatio: range(0, 0.449),
  yieldStrength: range(0, Infinity),
  ultimateStrength: range(0, Infinity),
  color,
  roughness: range(0, 1),
  metal: bool,
  clearcoat: bool,
  transparency: range(0, 1),
  emission: range(0, 1),
  refractiveIndex: range(1, 2.333),
}

function documentMaterial(doc, title) {
  const existing = doc.materials.find(material => material.title == title)
  if(existing) return existing
  const Preset = presets[title]
  if(!Preset) {
    const titles = [...new Set([...doc.materials.map(material => material.title), ...Object.keys(presets)])]
    throw new ToolError(`No material '${title}'. Use one of ${titles.join(', ')}, or edit the properties of an assigned material.`)
  }
  const material = new Preset()
  doc.materials.push(material)
  return material
}

function sharingComponents(doc, material) {
  return doc.top().getChildren().filter(component => !component.instanceOf && component.creator.material === material).map(itemId)
}

// Items that carry a name in the tree, with the component they belong to
function namedItem(doc, id) {
  const tree = doc.top()
  if(id.startsWith('sketch:')) {
    const sketch = findSketch(doc, id)
    return { component: sketchComponent(tree, sketch.id), key: sketch.id }
  }
  const kind = id.split(':')[0]
  if(storedKinds[kind]) return { stored: findStored(doc, id, kind).item }
  if(kind == 'feature') return { feature: findFeature(doc, id) }
  const item = resolveItem(doc, id)
  if(item.typename() == 'Component') return { componentItem: item }
  if(item.typename() == 'Solid') return { component: tree.findChild(item.compound.componentId), key: item.id }
  if(item instanceof ConstructionHelper) return { component: tree.findChild(item.componentId), key: item.id }
  throw new ToolError(`${id} has no name of its own`)
}

function variantOptionIndex(variant, value) {
  const index = Number.isInteger(value) ? value : variant.options.findIndex(option => option.title == value)
  if(!variant.options[index]) throw new ToolError(`No option ${JSON.stringify(value)}. Options are ${variant.options.map(option => option.title).join(', ')}.`)
  return index
}

async function loadCanvasImage(canvas, path, bus) {
  const { data, width, height } = await loadFile('image/*', 'dataUrl', path)
  Object.assign(canvas, { src: path, data, width, height })
  // As the canvas treelet loads images
  canvas.texture = new THREE.TextureLoader().load(data, () => bus.emit('render-needed'))
  canvas.texture.colorSpace = THREE.SRGBColorSpace
  canvas.texture.anisotropy = window.alcRenderer.renderer.capabilities.getMaxAnisotropy()
}

export const propertyTools = [
  {
    name: 'set_material',
    title: 'Set material',
    description: 'Assigns, edits or removes the material of a component, like the material treelet. Materials give bodies their mass, ' +
      'the stiffness and strength static load simulations need, and their rendered appearance. Components without a material use the one of their parent. ' +
      `Assign a material of the document by its title, or one of the presets ${Object.keys(presets).join(', ')}. ` +
      'Components can share a material, so changed properties apply to all of them, unless "copy" first gives the component a copy of its own. ' +
      'Units: density g/cm³, elasticModulus, yieldStrength and ultimateStrength MPa, color hex, roughness, transparency and emission 0 to 1.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Defaults to the active component' },
        material: { type: ['string', 'null'], description: 'Material or preset title to assign, null to remove the material' },
        copy: { type: 'boolean', description: 'Gives the component its own copy of its material before properties are changed' },
        properties: {
          type: 'object',
          description: `Material properties to change: ${Object.keys(materialProperties).join(', ')}`,
          additionalProperties: true,
        },
      },
    },
    run: async ({ component: id, material, copy, properties }, { app, doc }) => {
      editable(doc)
      await closeEditing(doc)
      const component = editableComponent(doc, id)
      const definition = component.creator
      if(material === null) definition.material = null
      else if(material !== undefined) definition.material = documentMaterial(doc, material)
      if((copy || properties) && !definition.material) {
        throw new ToolError('The component has no material of its own. Assign one first, the parent\'s material may be shared with other components.')
      }
      if(copy) {
        definition.material = definition.material.clone()
        doc.materials.push(definition.material)
      }
      if(properties) {
        const values = {}
        assign(values, properties, materialProperties, 'Materials')
        Object.assign(definition.material, values)
      }
      doc.markChanged()
      app.bus.emit('material-changed')
      app.bus.emit('render-needed')
      const effective = component.getMaterial()
      return {
        component: itemId(component),
        material: effective ? describeMaterial(effective) : null,
        ...(effective && !definition.material && { inheritedFrom: itemId(doc.top().getChildren().find(other => other.creator.material === effective && component.hasAncestor(other))) }),
        ...(definition.material && { sharedWith: sharingComponents(doc, definition.material).filter(other => other != itemId(component)) }),
      }
    },
  },
  {
    name: 'edit_item',
    title: 'Rename or recolor',
    description: 'Renames a component, body, sketch, construction plane, axis or point, feature, simulation, drawing or CAM setup, ' +
      'and sets the display color of components, which shows where they have no material.',
    inputSchema: {
      type: 'object',
      properties: {
        item: { type: 'string', description: 'Item id, like component:…, body:…, sketch:…, helper:…, feature:…, simulation:…, drawing:… or cam:…' },
        name: { type: 'string' },
        color: { type: 'string', description: 'Components only, hex color like #3080ff' },
      },
      required: ['item'],
    },
    run: async ({ item: id, name, color: newColor }, { app, doc }) => {
      editable(doc)
      await closeEditing(doc)
      if(name !== undefined) name = text(name, 'name')
      const { component, key, feature, stored, componentItem } = namedItem(doc, id)
      if(newColor !== undefined && !componentItem) throw new ToolError('Only components have a color')
      if(componentItem) {
        const source = editableComponent(doc, id)
        if(name) doc.renameComponent(source, name)
        if(newColor) {
          // Component colors are kept as HSL, which new components pick distinct colors from
          const { h, s, l } = new THREE.Color(color(newColor, 'color')).getHSL({}, THREE.SRGBColorSpace)
          source.creator.color = `hsl(${(h * 360).toFixed(1)}, ${(s * 100).toFixed(1)}%, ${(l * 100).toFixed(1)}%)`
          source.creator.updateMaterials()
          app.bus.emit('render-needed')
        }
        doc.markChanged()
        return { item: itemId(source), name: source.creator.title, color: '#' + new THREE.Color(source.creator.color).getHexString() }
      }
      if(!name) throw new ToolError('Give a name')
      if(feature) feature.title = name
      else if(stored) {
        if(!('title' in stored)) throw new ToolError(`${id} has no name`)
        stored.title = name
      } else {
        if(component.readOnly) throw new ToolError(`${id} belongs to a read-only component`)
        doc.renameTreeItem(component, key, name)
      }
      doc.markChanged()
      return { item: id, name }
    },
  },
  {
    name: 'edit_variant',
    title: 'Edit configurations',
    description: 'Manages the configurations (variant options) of a component. Each option stores a value for every parameter of the component: ' +
      'set_parameter changes the values of the active option, and activating another option applies its values and regenerates the model. ' +
      'Actions apply in the order create, addOption, renameOption, activate, removeOption, remove.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Defaults to the active component' },
        create: { type: 'boolean', description: 'Adds configurations to the component, with the current parameter values as first option' },
        addOption: { type: 'string', description: 'Adds an option with the current parameter values under this title and activates it' },
        renameOption: {
          type: 'object',
          properties: { option: { type: ['string', 'integer'] }, title: { type: 'string' } },
          required: ['option', 'title'],
        },
        activate: { type: ['string', 'integer'], description: 'Option title or index to activate' },
        removeOption: { type: ['string', 'integer'] },
        remove: { type: 'boolean', description: 'Removes the configurations, keeping the active values' },
      },
    },
    run: async ({ component: id, create, addOption, renameOption, activate, removeOption, remove }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      const component = editableComponent(doc, id)
      const definition = component.creator
      const currentValues = () => Object.fromEntries(definition.parameters.map(parameter => [parameter.id, parameter.value]))
      if(create) {
        if(definition.variant) throw new ToolError('The component has configurations already')
        definition.variant = { activeOption: 0, options: [{ title: 'Option 1', values: currentValues() }] }
      }
      const variant = definition.variant
      if(!variant) throw new ToolError('The component has no configurations, use create first')
      if(addOption !== undefined) {
        definition.syncActiveVariantOption()
        variant.options.push({ title: text(addOption, 'addOption'), values: currentValues() })
        variant.activeOption = variant.options.length - 1
      }
      if(renameOption) variant.options[variantOptionIndex(variant, renameOption.option)].title = text(renameOption.title, 'title')
      if(activate !== undefined) definition.selectVariantOption(variantOptionIndex(variant, activate))
      if(removeOption !== undefined) {
        if(variant.options.length == 1) throw new ToolError('The last option cannot be removed, remove the configurations instead')
        variant.options.splice(variantOptionIndex(variant, removeOption), 1)
        variant.activeOption = Math.min(variant.activeOption, variant.options.length - 1)
        definition.selectVariantOption(variant.activeOption, false)
      }
      if(remove) definition.variant = null
      doc.markChanged()
      await doc.regenerateParameters()
      await settle(doc)
      return {
        component: itemId(component),
        variant: describeVariant(component),
        parameters: definition.parameters.map(({ name, value }) => ({ name, value })),
      }
    },
  },
  {
    name: 'edit_canvas',
    title: 'Edit canvas',
    description: 'Adds, changes or removes a canvas: a reference image shown on an origin plane of a component, to model after. ' +
      'Without a canvas id a new one is added, which needs an image.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'For new canvases, defaults to the active component' },
        canvas: { type: 'string', description: 'Existing canvas, like canvas:id-5' },
        image: { type: 'string', description: 'Absolute path of an image file' },
        plane: { type: 'string', enum: ['top', 'front', 'side'], description: 'Top is the XZ, front the XY and side the YZ plane' },
        scale: { type: 'number', description: 'Image pixels to mm' },
        x: { type: 'number', description: 'Horizontal offset on the plane in mm' },
        y: { type: 'number', description: 'Vertical offset on the plane in mm' },
        transparency: { type: 'number', description: '0 to 1' },
        hidden: { type: 'boolean' },
        remove: { type: 'boolean' },
      },
    },
    run: async ({ component: componentId, canvas: canvasId, image, remove, ...settings }, { app, doc }) => {
      editable(doc)
      await closeEditing(doc)
      let component, canvas
      if(canvasId) {
        ({ component, item: canvas } = findStored(doc, canvasId, 'canvas'))
        component = editableComponent(doc, itemId(component))
      } else {
        if(!image) throw new ToolError('A new canvas needs an image')
        component = editableComponent(doc, componentId)
        canvas = new Canvas()
      }
      if(remove) {
        doc.activateInteractivePanel(null)
        component.creator.canvases = component.creator.canvases.filter(other => other !== canvas)
        doc.markChanged()
        return { removed: canvasId }
      }
      const values = {}
      assign(values, settings, {
        plane: value => option(value, ['top', 'front', 'side'], 'plane'),
        scale: range(1e-6, Infinity),
        x: range(-Infinity, Infinity),
        y: range(-Infinity, Infinity),
        transparency: range(0, 1),
        hidden: bool,
      }, 'Canvases')
      if(image) await loadCanvasImage(canvas, image, app.bus)
      Object.assign(canvas, values)
      if(!canvasId) component.creator.canvases.push(canvas)
      doc.markChanged()
      app.bus.emit('render-needed')
      return describeStored('canvas', canvas)
    },
  },
]
