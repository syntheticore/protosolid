import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { resolveSimulationFaces } from './simulation.js'
import { findVoxelNode } from './voxel.js'
import { traceFlowLines } from './fluid-lines.js'


export const fluidMaterials = {
  air: { label: 'Air (20 °C)', density: 1.204, viscosity: 0.0000181 },
  water: { label: 'Water (20 °C)', density: 998.2, viscosity: 0.001002 },
  custom: { label: 'Custom fluid / gas' },
}

export class FluidSimulation {
  constructor(id) {
    this.id = id || makeID()
    this.mode = 'fluid'
    this.title = 'Fluid Flow'
    this.fluid = 'air'
    this.density = fluidMaterials.air.density
    this.viscosity = fluidMaterials.air.viscosity
    this.inletSpeed = 1
    this.outletPressure = 0
    this.resolution = 16
    this.inletFaces = []
    this.outletFaces = []
    this.showFlowVectors = true
    this.showFlowLines = true
    this.result = null
    this.revision = 0
  }

  setFluid(fluid) {
    this.fluid = fluid
    if(fluid != 'custom') {
      this.density = fluidMaterials[fluid].density
      this.viscosity = fluidMaterials[fluid].viscosity
    }
  }

  resolve(tree, bind=true) {
    if(!this.inletFaces.length || !this.outletFaces.length) return 'Pick inlet and outlet faces on the fluid volume'
    const [inlets, outlets] = [this.inletFaces, this.outletFaces].map(references =>
      bind ? references : references.map(reference => reference.clone()))
    const problem = resolveSimulationFaces(tree, [inlets, outlets])
    if(problem) return problem
    if(inlets.some(inlet => outlets.some(outlet => inlet.getItem() === outlet.getItem()))) {
      return 'A face cannot be both an inlet and an outlet'
    }
    if(!Number.isFinite(this.inletSpeed) || this.inletSpeed <= 0) return 'Enter a positive inlet speed'
    if(!Number.isFinite(this.density) || this.density <= 0) return 'Enter a positive density'
    if(!Number.isFinite(this.viscosity) || this.viscosity <= 0) return 'Enter a positive viscosity'
    if(!Number.isFinite(this.outletPressure)) return 'Enter an outlet pressure'
    if(!Number.isInteger(this.resolution) || this.resolution < 8 || this.resolution > 24) return 'Grid cells must be an integer from 8 to 24'
  }

  sampleAt(position) {
    if(!this.result) return
    const node = findVoxelNode(this.result, position)
    if(node === undefined) return
    const velocity = new THREE.Vector3().fromArray(this.result.velocities, node * 3)
    return { velocity, speed: velocity.length(), pressure: this.result.pressures[node] }
  }

  flowSamples() {
    if(!this.result) return []
    const result = this.result
    const samples = []
    for(let k = 0; k < result.nz; k += 2) {
      for(let j = 0; j < result.ny; j += 2) {
        for(let i = 0; i < result.nx; i += 2) {
          const node = result.grid[i + result.nx * (j + result.ny * k)]
          if(node < 0) continue
          const flux = new THREE.Vector3().fromArray(result.velocities, node * 3)
          if(flux.length() < result.maxSpeed * 0.01) continue
          samples.push({
            position: new THREE.Vector3(
              result.min[0] + (i + 0.5) * result.step,
              result.min[1] + (j + 0.5) * result.step,
              result.min[2] + (k + 0.5) * result.step,
            ),
            flux,
          })
        }
      }
    }
    return samples
  }

  flowLines() {
    return this.result ? traceFlowLines(this.result) : []
  }

  dump() {
    return Object.fromEntries([
      'id', 'title', 'fluid', 'density', 'viscosity', 'inletSpeed', 'outletPressure',
      'resolution', 'inletFaces', 'outletFaces', 'showFlowVectors', 'showFlowLines',
    ].map(key => [key, this[key]]))
  }

  static undump(dump) {
    return Object.assign(new FluidSimulation(dump.id), dump)
  }
}
Serialize.register(FluidSimulation, 'FluidSimulation')
