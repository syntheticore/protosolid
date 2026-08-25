import * as THREE from 'three'

const GRID_MULTIPLE = 3
const GRID_DIVISIONS = 10 * GRID_MULTIPLE

function gridMaterial(size) {
  return new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    vertexColors: true,
    uniforms: {
      opacity: { value: 0.16 },
      gridSize: { value: size * GRID_MULTIPLE },
      hasMaskBounds: { value: false },
      maskBounds: { value: new THREE.Vector4() },
      maskInnerRadius: { value: size * 0.2 },
      maskOuterRadius: { value: size * 0.7 },
    },
    vertexShader: /* glsl */`
      varying vec3 vColor;
      varying vec2 vGridPosition;

      void main() {
        vColor = color;
        // GridHelper is created in XZ and rotated onto the sketch's XY plane.
        vGridPosition = vec2(position.x, -position.z);
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */`
      uniform float opacity;
      uniform float gridSize;
      uniform bool hasMaskBounds;
      uniform vec4 maskBounds;
      uniform float maskInnerRadius;
      uniform float maskOuterRadius;

      varying vec3 vColor;
      varying vec2 vGridPosition;

      void main() {
        float distanceToSketch = length(vGridPosition);
        if(hasMaskBounds) {
          vec2 outside = max(
            max(maskBounds.xy - vGridPosition, vGridPosition - maskBounds.zw),
            vec2(0.0)
          );
          distanceToSketch = length(outside);
        }

        float sketchFade = 1.0 - smoothstep(
          maskInnerRadius,
          maskOuterRadius,
          distanceToSketch
        );

        // GridHelper has a hard square boundary. Fade before reaching it so
        // the last rendered line never exposes that boundary.
        float edgeDistance = max(abs(vGridPosition.x), abs(vGridPosition.y));
        float halfSize = gridSize * 0.5;
        float edgeFade = 1.0 - smoothstep(halfSize * 0.72, halfSize, edgeDistance);
        float alpha = opacity * sketchFade * edgeFade;
        if(alpha <= 0.001) discard;

        gl_FragColor = vec4(vColor, alpha);
        #include <tonemapping_fragment>
        #include <colorspace_fragment>
      }
    `,
  })
}

export default class SketchPlane extends THREE.Object3D {
  constructor(camera) {
    super()

    // Grid
    this.cachedVec = new THREE.Vector3()
    this.maskBounds = new THREE.Box2()
    this.maskSignature = null
    this.sketch = null
    this.update(camera)

    // Axis Helper
    this.add(new THREE.AxesHelper(10.0))

    // Click Catcher
    var groundGeo = new THREE.PlaneGeometry(99999, 99999)
    var ground = new THREE.Mesh(groundGeo, new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.0,
      depthWrite: false,
    }))
    ground.alcProjectable = true
    this.add(ground)

    this.visible = false
  }

  setPlane(plane) {
    this.position.setFromMatrixPosition(plane)
    this.rotation.setFromRotationMatrix(plane)
  }

  setSketch(sketch) {
    this.sketch = sketch
    this.maskSignature = null
    this.updateMask()
  }

  sketchElements() {
    if(!this.sketch) return []
    const projections = this.sketch.projections
      .map(projection => projection.geometry())
      .filter(Boolean)
    return [...this.sketch.elements, ...projections]
  }

  geometrySignature(elements) {
    return JSON.stringify(elements.map(element => ({
      id: element.id,
      point: element.point,
      points: element.points,
      center: element._center,
      radius: element.radius,
      bounds: element.bounds,
      forward: element.forward,
    })))
  }

  sampleSketch(elements) {
    const points = elements.flatMap(element => {
      if(!element.sample || !element.geom) {
        return (element.handles?.() || []).map(point => point.clone())
      }

      // Curves are sampled only when their geometry changes. These samples
      // establish one continuous footprint for the whole sketch.
      const count = Math.min(16, Math.max(2, Math.ceil(element.tesselationSteps() / 8)))
      try {
        return Array.from({ length: count }, (_unused, index) =>
          element.sample(count == 1 ? 0 : index / (count - 1))
        ).filter(Boolean)
      } catch(_error) {
        return (element.handles?.() || []).map(point => point.clone())
      }
    }).filter(point => Number.isFinite(point.x) && Number.isFinite(point.y))

    return points
  }

  updateMask() {
    if(!this.grid) return
    const elements = this.sketchElements()
    const signature = this.geometrySignature(elements)
    if(signature != this.maskSignature) {
      const points = this.sampleSketch(elements)
      this.maskBounds.makeEmpty()
      points.forEach(point => this.maskBounds.expandByPoint(point))
      this.maskSignature = signature
    }
    const uniforms = this.grid.material.uniforms
    uniforms.hasMaskBounds.value = !this.maskBounds.isEmpty()
    if(!this.maskBounds.isEmpty()) {
      uniforms.maskBounds.value.set(
        this.maskBounds.min.x,
        this.maskBounds.min.y,
        this.maskBounds.max.x,
        this.maskBounds.max.y,
      )
    }
  }

  update(camera) {
    const relPos = camera.position.clone().sub(this.position)
    const up = this.up.clone().applyEuler(this.rotation)
    const dist = Math.abs(up.dot(relPos))
    const dot = Math.abs(up.dot(relPos.normalize()))
    const size = Math.pow(10, String(Math.round(10 * dist / 4)).length) / 10
    if(size != this.lastSize && dot > 0.5) {
      if(this.grid) {
        this.remove(this.grid)
        this.grid.geometry.dispose()
        this.grid.material.dispose()
      }
      this.grid = new THREE.GridHelper(size * GRID_MULTIPLE, GRID_DIVISIONS)
      this.grid.rotateX(Math.PI / 2)
      this.grid.material.dispose()
      this.grid.material = gridMaterial(size)
      this.add(this.grid)
      this.lastSize = size
    }
    this.updateMask()

    // Move grid towards camera to avoid z-fighting
    const lookVec = camera.getWorldDirection(this.cachedVec)
    const facing = this.up.dot(lookVec)
    this.grid.position.z = -0.004 * dist * facing
  }
}
