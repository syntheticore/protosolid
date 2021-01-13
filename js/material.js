import * as THREE from 'three'

export class Material {
  constructor(title) {
    this.title = title
    this.density = 1.0 // g/cm^3
    this.displayMaterial = new THREE.MeshStandardMaterial({
      side: THREE.DoubleSide, //XXX remove
    })
  }

  get color() {
    return '#' + this.displayMaterial.color.getHexString()
  }

  get roughness() {
    return this.displayMaterial.roughness
  }

  get metal() {
    return this.displayMaterial.metalness > 0.5
  }

  get transparency() {
    return 1.0 - this.displayMaterial.opacity
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
    this.displayMaterial.metalness = bool ? 1.0 : 0.0
  }

  set transparency(transparency) {
    this.displayMaterial.opacity = 1.0 - transparency
    this.displayMaterial.transparent = (this.displayMaterial.opacity < 0.9999999)
  }

  set translucency(translucency) {
    this.glow = translucency
    this.displayMaterial.emissive = this.displayMaterial.color.clone().multiplyScalar(translucency)
  }
}

export class Aluminum extends Material {
  constructor() {
    super('Aluminum')

    this.density = 2.7

    this.color = '#abc5d9'
    this.roughness = 0.15
    this.metal = true
  }
}

export class Polystyrene extends Material {
  constructor() {
    super('Polystyrene')

    this.density = 0.9

    this.color = '#ff57a3'
    this.roughness = 0.3
    this.translucency = 0.15
  }
}


// 0.9 Polystyrene
// 1.1 Rubber
// 1.8 Brick
// 2.2 Glass
// 2.7 Aluminum
// 7.2 Cast Iron
// 8.4 Brass
// 11.3 Lead
// 19.3 Gold
