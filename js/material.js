import * as THREE from 'three'
import { markRaw } from 'vue'
import Serialize from './core/serialize.js'

export class Material {
  constructor(title) {
    this.title = title
    this.density = 1.0 // g/cm^3
    this.elasticModulus = null // MPa = N/mm²
    this.poissonRatio = null
    this.yieldStrength = null // MPa, onset of plastic deformation
    this.ultimateStrength = null // MPa, fracture
    // Three.js materials contain mutable render state and should not be
    // deeply proxied when the owning document is made reactive by Vue.
    this.displayMaterial = markRaw(new THREE.MeshPhysicalMaterial({
      side: THREE.DoubleSide,
      clearcoatRoughness: 0.05,
      iridescence: 0.25,
      sheen: 0.25,
      // Nonzero thickness makes the path tracer refract at each solid boundary.
      thickness: 1,
    }))
  }

  get color() {
    return '#' + this.displayMaterial.color.getHexString()
  }

  get roughness() {
    return this.displayMaterial.roughness
  }

  get metal() {
    return this.isMetal
  }

  get clearcoat() {
    return this.displayMaterial.clearcoat > 0.5
  }

  get transparency() {
    return this.displayMaterial.transmission
  }

  get emission() {
    return this.glow || 0.0
  }

  get refractiveIndex() {
    return this.displayMaterial.ior
  }

  set color(color) {
    this.displayMaterial.color = new THREE.Color(color)
    this.emission = this.glow || 0.0
  }

  set roughness(roughness) {
    this.displayMaterial.roughness = roughness
  }

  set metal(bool) {
    this.isMetal = bool
    this.displayMaterial.metalness = bool ? 1.0 : 0.0
  }

  set clearcoat(bool) {
    this.displayMaterial.clearcoat = bool ? 1.0 : 0.0
  }

  set transparency(transparency) {
    // Alpha transparency skips surfaces, including their reflections. Physical
    // transmission preserves the surface and lets IOR control the bending.
    this.displayMaterial.transmission = Number(transparency)
  }

  set emission(emission) {
    this.glow = emission
    this.displayMaterial.emissive = this.displayMaterial.color.clone().multiplyScalar(emission)
  }

  set refractiveIndex(refractiveIndex) {
    this.displayMaterial.ior = Number(refractiveIndex)
  }

  clone() {
    const out = new Material(this.title)
    out.density = this.density
    out.elasticModulus = this.elasticModulus
    out.poissonRatio = this.poissonRatio
    out.yieldStrength = this.yieldStrength
    out.ultimateStrength = this.ultimateStrength
    out.color = this.color
    out.roughness = this.roughness
    out.metal = this.metal
    out.clearcoat = this.clearcoat
    out.transparency = this.transparency
    out.emission = this.emission
    out.refractiveIndex = this.refractiveIndex
    return out
  }

  dump() {
    return {
      title: this.title,
      density: this.density,
      elasticModulus: this.elasticModulus,
      poissonRatio: this.poissonRatio,
      yieldStrength: this.yieldStrength,
      ultimateStrength: this.ultimateStrength,
      color: this.color,
      roughness: this.roughness,
      metal: this.metal,
      clearcoat: this.clearcoat,
      transparency: this.transparency,
      emission: this.emission,
      refractiveIndex: this.refractiveIndex,
    }
  }

  static undump(dump, context) {
    const material = new this(dump.title)
    material.title = dump.title
    material.density = dump.density
    material.elasticModulus = dump.elasticModulus ?? material.elasticModulus
    material.poissonRatio = dump.poissonRatio ?? material.poissonRatio
    material.yieldStrength = dump.yieldStrength ?? material.yieldStrength
    material.ultimateStrength = dump.ultimateStrength ?? material.ultimateStrength
    material.color = dump.color
    material.roughness = dump.roughness
    material.metal = dump.metal
    material.clearcoat = dump.clearcoat
    material.transparency = dump.transparency
    material.emission = dump.emission ?? dump.translucency ?? 0
    material.refractiveIndex = dump.refractiveIndex ?? 1.5
    if(context?.materials) context.materials.push(material)
    return material
  }

  dispose() {
    this.displayMaterial.dispose()
  }
}

export class Aluminum extends Material {
  constructor() {
    super('Aluminum')

    this.density = 2.7
    this.elasticModulus = 69000
    this.poissonRatio = 0.33
    this.yieldStrength = 276
    this.ultimateStrength = 310

    this.metal = true
    this.color = '#abc5d9'
    this.roughness = 0.15
  }
}

export class DarkViolet extends Material {
  constructor() {
    super('Aluminum Anodized')

    this.density = 2.7
    this.elasticModulus = 69000
    this.poissonRatio = 0.33
    this.yieldStrength = 276
    this.ultimateStrength = 310

    this.metal = true
    this.color = '#1a1835'
    this.roughness = 0.5
    this.emission = 0.13
  }
}

export class Polystyrene extends Material {
  constructor() {
    super('Polystyrene')

    this.density = 0.9
    this.elasticModulus = 3000
    this.poissonRatio = 0.35
    this.ultimateStrength = 40

    this.color = '#ff57a3'
    this.roughness = 0.3
    this.emission = 0.12
  }
}

export class CarPaint extends Material {
  constructor() {
    super('Car Paint')

    this.density = 2.7

    this.metal = true
    this.color = '#740606'
    this.roughness = 0.386
    this.clearcoat = true
  }
}

export class Glass extends Material {
  constructor() {
    super('Glass')

    this.density = 2.2
    this.elasticModulus = 70000
    this.poissonRatio = 0.22
    this.ultimateStrength = 40

    this.color = 'white'
    this.roughness = 0.0
    this.transparency = 0.98
  }
}

Serialize.register(Material, 'Material')
Serialize.register(Aluminum, 'Aluminum')
Serialize.register(DarkViolet, 'DarkViolet')
Serialize.register(Polystyrene, 'Polystyrene')
Serialize.register(CarPaint, 'CarPaint')
Serialize.register(Glass, 'Glass')


// 0.9 Polystyrene
// 1.0 Water
// 1.1 Rubber
// 1.8 Brick
// 2.2 Glass
// 2.7 Aluminum
// 7.2 Cast Iron
// 7.85 Steel
// 8.4 Brass
// 11.3 Lead
// 19.3 Gold
