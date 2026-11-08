import * as THREE from 'three'

import { currentTheme, onThemeChange } from '../theme.js'

// Colors that need to contrast the viewport background of each theme
const themeColors = {
  dark: { feed: '#3fa9f5', rapid: '#ff5a4e', outline: '#c0c8d0', fixture: '#4a5560', fixtureMetalness: 0.7 },
  // Less metallic, as unlit metal turns black against the bright background
  light: { feed: '#1683d8', rapid: '#e0352a', outline: '#6f767e', fixture: '#a3acb5', fixtureMetalness: 0.3 },
}

// Toolpaths, stock and tool of a CAM setup, in machine coordinates
export default class CamView extends THREE.Group {
  constructor(result) {
    super()
    this.result = result
    const { width, depth, height } = result.grid

    // Toolpaths
    const positions = []
    const moves = result.moves
    for(let i = 1; i < moves.length; i++) {
      const a = moves[i - 1]
      const b = moves[i]
      positions.push(a.x, a.y, a.z, b.x, b.y, b.z)
    }
    const pathGeometry = new THREE.BufferGeometry()
    pathGeometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3))
    pathGeometry.setAttribute('color', new THREE.Float32BufferAttribute(positions.length, 3))
    this.paths = new THREE.LineSegments(pathGeometry, new THREE.LineBasicMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.6,
    }))
    this.add(this.paths)

    // Stock outline
    const box = new THREE.BoxGeometry(width, depth, height).translate(width / 2, depth / 2, -height / 2)
    this.outline = new THREE.LineSegments(new THREE.EdgesGeometry(box), new THREE.LineBasicMaterial())
    box.dispose()
    this.add(this.outline)

    // Vise jaws and tool holders
    this.fixtureMaterial = new THREE.MeshStandardMaterial({ roughness: 0.5 })

    this.add(this.createVise())

    // Work zero, where the machine is touched off
    const origin = new THREE.AxesHelper(Math.min(width, depth) / 3)
    origin.position.set(result.origin.x, result.origin.y, 0)
    origin.material.depthTest = false
    origin.renderOrder = 10
    this.add(origin)

    this.stock = this.createStock()
    this.add(this.stock)
    this.tools = result.operations.map(({ tool }) => {
      const object = createTool(tool, this.fixtureMaterial)
      this.add(object)
      return object
    })

    this.applyTheme(currentTheme())
    this.unsubscribeTheme = onThemeChange(theme => this.applyTheme(theme))
  }

  applyTheme(theme) {
    const colors = themeColors[theme]
    const feed = new THREE.Color(colors.feed)
    const rapid = new THREE.Color(colors.rapid)
    const attribute = this.paths.geometry.attributes.color
    this.result.moves.slice(1).forEach((move, i) => {
      const color = move.rapid ? rapid : feed
      attribute.setXYZ(i * 2, color.r, color.g, color.b)
      attribute.setXYZ(i * 2 + 1, color.r, color.g, color.b)
    })
    attribute.needsUpdate = true
    this.outline.material.color.set(colors.outline)
    this.fixtureMaterial.color.set(colors.fixture)
    this.fixtureMaterial.metalness = colors.fixtureMetalness
  }

  // Jaws gripping the front and back of the stock below the lowest cut
  createVise() {
    const { width, depth, height, floor } = this.result.grid
    const vise = new THREE.Group()
    const grip = floor - 1 + height
    if(grip <= 0) return vise
    const material = this.fixtureMaterial
    const thickness = 12
    const jawHeight = grip + 15
    for(const y of [-thickness / 2, depth + thickness / 2]) {
      const jaw = new THREE.Mesh(new THREE.BoxGeometry(width + 30, thickness, jawHeight), material)
      jaw.position.set(width / 2, y, floor - 1 - jawHeight / 2)
      vise.add(jaw)
    }
    return vise
  }

  // Height field of the stock, closed by a ring at the stock bottom
  createStock() {
    const { cell, nx, ny, width, depth, height } = this.result.grid
    const columns = nx + 2
    const rows = ny + 2
    const positions = new Float32Array(columns * rows * 3)
    const coordinate = (index, count, size) =>
      index == 0 ? 0 : index == count + 1 ? size : (index - 0.5) * cell
    for(let J = 0; J < rows; J++) {
      for(let I = 0; I < columns; I++) {
        const index = (I + columns * J) * 3
        positions[index] = coordinate(I, nx, width)
        positions[index + 1] = coordinate(J, ny, depth)
        positions[index + 2] = -height
      }
    }
    const indices = []
    for(let J = 0; J < rows - 1; J++) {
      for(let I = 0; I < columns - 1; I++) {
        const a = I + columns * J
        const b = a + 1
        const c = a + 1 + columns
        const d = a + columns
        indices.push(a, b, c, a, c, d)
      }
    }
    const geometry = new THREE.BufferGeometry()
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3))
    geometry.setIndex(indices)
    const material = new THREE.MeshStandardMaterial({
      color: '#d4d9de',
      metalness: 0.4,
      roughness: 0.45,
      side: THREE.DoubleSide,
    })
    const stock = new THREE.Mesh(geometry, material)
    const bottom = new THREE.Mesh(new THREE.PlaneGeometry(width, depth), material)
    bottom.position.set(width / 2, depth / 2, -height)
    stock.add(bottom)
    return stock
  }

  update(simulation) {
    // The paths would hide the cut
    this.paths.visible = !simulation
    this.outline.visible = !simulation
    this.stock.visible = !!simulation
    this.tools.forEach(tool => tool.visible = false)
    if(!simulation) return

    const { nx, ny } = this.result.grid
    const columns = nx + 2
    const position = this.stock.geometry.attributes.position
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        position.array[(i + 1 + columns * (j + 1)) * 3 + 2] = simulation.heights[i + nx * j]
      }
    }
    position.needsUpdate = true
    this.stock.geometry.computeVertexNormals()

    const tool = this.tools[simulation.position.op]
    tool.visible = true
    tool.position.set(simulation.position.x, simulation.position.y, simulation.position.z)
  }

  dispose() {
    this.unsubscribeTheme()
    this.traverse(object => {
      object.geometry?.dispose()
      object.material?.dispose()
    })
  }
}

// End mill or drill and holder, with the tool tip at the origin
function createTool({ radius, ball, drill }, holderMaterial) {
  const group = new THREE.Group()
  const cutterMaterial = new THREE.MeshStandardMaterial({ color: '#d6b25a', metalness: 0.9, roughness: 0.3 })
  const flute = Math.max(radius * (drill ? 10 : 6), 10)
  // Length of a 118° drill point
  const point = radius / Math.tan(THREE.MathUtils.degToRad(59))
  const start = ball ? radius : drill ? point : 0

  const cutter = new THREE.Mesh(new THREE.CylinderGeometry(radius, radius, flute - start, 24), cutterMaterial)
  cutter.rotation.x = Math.PI / 2
  cutter.position.z = start + (flute - start) / 2
  group.add(cutter)

  if(ball) {
    const tip = new THREE.Mesh(new THREE.SphereGeometry(radius, 24, 12), cutterMaterial)
    tip.position.z = radius
    group.add(tip)
  }

  if(drill) {
    const tip = new THREE.Mesh(new THREE.ConeGeometry(radius, point, 24), cutterMaterial)
    tip.rotation.x = -Math.PI / 2
    tip.position.z = point / 2
    group.add(tip)
  }

  const holderRadius = radius * 1.8
  const holder = new THREE.Mesh(new THREE.CylinderGeometry(holderRadius, holderRadius, 20, 32), holderMaterial)
  holder.rotation.x = Math.PI / 2
  holder.position.z = flute + 10
  group.add(holder)

  group.visible = false
  return group
}
