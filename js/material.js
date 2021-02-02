import * as THREE from 'three'

export class Material {
  constructor(title) {
    this.title = title
    this.density = 1.0 // g/cm^3
    this.displayMaterial = new THREE.MeshPhysicalMaterial({
      clearcoatRoughness: 0.05,
    })
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

  get translucency() {
    return this.glow || 0.0
  }

  set color(color) {
    this.displayMaterial.color = new THREE.Color(color)
    this.translucency = this.glow || 0.0
  }

  set roughness(roughness) {
    this.displayMaterial.roughness = roughness
  }

  set metal(bool) {
    this.isMetal = bool
    this.displayMaterial.metalness = bool ? 1.0 - this.displayMaterial.transmission : 0.0
  }

  set clearcoat(bool) {
    this.displayMaterial.clearcoat = bool ? 1.0 : 0.0
  }

  set transparency(transparency) {
    this.displayMaterial.transmission = transparency
    this.displayMaterial.transparent = (this.displayMaterial.transmission > 0.000001)
    this.metal = this.isMetal
  }

  set translucency(translucency) {
    this.glow = translucency
    this.displayMaterial.emissive = this.displayMaterial.color.clone().multiplyScalar(translucency)
  }

  dispose() {
    this.displayMaterial.dispose()
  }
}

export class Aluminum extends Material {
  constructor() {
    super('Aluminum Smooth')

    this.density = 2.7

    this.metal = true
    this.color = '#abc5d9'
    this.roughness = 0.15
  }
}

export class DarkViolet extends Material {
  constructor() {
    super('Aluminum Anodized')

    this.density = 2.7

    this.metal = true
    this.color = '#1a1835'
    this.roughness = 0.5
    this.translucency = 0.13
  }
}

export class Polystyrene extends Material {
  constructor() {
    super('Polystyrene')

    this.density = 0.9

    this.color = '#ff57a3'
    this.roughness = 0.3
    this.translucency = 0.12
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

    this.color = 'white'
    this.roughness = 0.0
    this.transparency = 0.98
  }
}


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
