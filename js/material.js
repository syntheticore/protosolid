export default class Material {
  constructor(title) {
    this.title = title

    this.color = '#53a3e1'
    this.roughness = 0.25
    this.metalness = 0.2
    this.opacity = 0.0
    this.translucency = 0.0

    this.density = 2.7 // g/cm^3
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
