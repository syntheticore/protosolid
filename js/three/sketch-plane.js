import * as THREE from 'three'

const VIRTUAL_GRID_SIZE = 99999
const MAX_HULL_POINTS = 64

function cross(origin, a, b) {
  return (a.x - origin.x) * (b.y - origin.y) -
    (a.y - origin.y) * (b.x - origin.x)
}

function convexHull(points) {
  const sorted = points
    .map(point => new THREE.Vector2(point.x, point.y))
    .sort((a, b) => a.x - b.x || a.y - b.y)
    .filter((point, index, all) =>
      index == 0 || point.distanceToSquared(all[index - 1]) > 1.0e-12
    )
  if(sorted.length <= 2) return sorted

  const lower = []
  sorted.forEach(point => {
    while(lower.length >= 2 && cross(lower.at(-2), lower.at(-1), point) <= 0) lower.pop()
    lower.push(point)
  })

  const upper = []
  const reversed = [...sorted].reverse()
  reversed.forEach(point => {
    while(upper.length >= 2 && cross(upper.at(-2), upper.at(-1), point) <= 0) upper.pop()
    upper.push(point)
  })

  const hull = [...lower.slice(0, -1), ...upper.slice(0, -1)]
  if(hull.length <= MAX_HULL_POINTS) return hull
  return Array.from({ length: MAX_HULL_POINTS }, (_unused, index) =>
    hull[Math.floor(index * hull.length / MAX_HULL_POINTS)]
  )
}

function gridMaterial(size) {
  return new THREE.ShaderMaterial({
    side: THREE.DoubleSide,
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    uniforms: {
      opacity: { value: 0.16 },
      color: { value: new THREE.Color(0x888888) },
      gridStep: { value: size / 10 },
      maskHullCount: { value: 0 },
      maskHull: {
        value: Array.from({ length: MAX_HULL_POINTS }, () => new THREE.Vector2()),
      },
      maskInnerRadius: { value: size * 0.2 },
      maskOuterRadius: { value: size * 0.7 },
    },
    vertexShader: /* glsl */`
      varying vec2 vGridPosition;

      void main() {
        vGridPosition = position.xy;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */`
      #define MAX_HULL_POINTS ${MAX_HULL_POINTS}

      uniform float opacity;
      uniform vec3 color;
      uniform float gridStep;
      uniform int maskHullCount;
      uniform vec2 maskHull[MAX_HULL_POINTS];
      uniform float maskInnerRadius;
      uniform float maskOuterRadius;

      varying vec2 vGridPosition;

      float distanceToSegment(vec2 point, vec2 a, vec2 b) {
        vec2 segment = b - a;
        float lengthSquared = dot(segment, segment);
        if(lengthSquared <= 1.0e-12) return distance(point, a);
        float position = clamp(dot(point - a, segment) / lengthSquared, 0.0, 1.0);
        return distance(point, a + segment * position);
      }

      float edgeCross(vec2 point, vec2 a, vec2 b) {
        vec2 edge = b - a;
        vec2 relative = point - a;
        return edge.x * relative.y - edge.y * relative.x;
      }

      void main() {
        // Draw an anti-aliased grid procedurally. The backing plane is large,
        // but the number of rendered primitives no longer grows with it.
        vec2 gridCoordinate = vGridPosition / gridStep;
        vec2 lineDistance = abs(fract(gridCoordinate - 0.5) - 0.5) /
          max(fwidth(gridCoordinate), vec2(1.0e-6));
        float gridLine = 1.0 - min(min(lineDistance.x, lineDistance.y), 1.0);

        float distanceToSketch = length(vGridPosition);
        if(maskHullCount == 1) {
          distanceToSketch = distance(vGridPosition, maskHull[0]);
        } else if(maskHullCount > 1) {
          float hullDistance = 1.0e20;
          bool insideHull = maskHullCount >= 3;
          vec2 first = maskHull[0];
          vec2 previous = first;

          for(int i = 1; i < MAX_HULL_POINTS; i++) {
            if(i >= maskHullCount) break;
            vec2 current = maskHull[i];
            hullDistance = min(
              hullDistance,
              distanceToSegment(vGridPosition, previous, current)
            );
            insideHull = insideHull && edgeCross(vGridPosition, previous, current) >= -1.0e-5;
            previous = current;
          }

          hullDistance = min(
            hullDistance,
            distanceToSegment(vGridPosition, previous, first)
          );
          insideHull = insideHull && edgeCross(vGridPosition, previous, first) >= -1.0e-5;
          distanceToSketch = insideHull ? 0.0 : hullDistance;
        }

        float sketchFade = 1.0 - smoothstep(
          maskInnerRadius,
          maskOuterRadius,
          distanceToSketch
        );

        float alpha = opacity * gridLine * sketchFade;
        if(alpha <= 0.001) discard;

        gl_FragColor = vec4(color, alpha);
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
    this.cachedNormal = new THREE.Vector3()
    this.maskHull = []
    this.maskSignature = null
    this.sketch = null
    this.update(camera)

    // Axis Helper
    this.add(new THREE.AxesHelper(10.0))

    // Click Catcher
    var groundGeo = new THREE.PlaneGeometry(VIRTUAL_GRID_SIZE, VIRTUAL_GRID_SIZE)
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
      start: element.start,
      sweep: element.sweep,
      trim: element.trim,
    })))
  }

  sampleSketch(elements) {
    const points = elements.flatMap(element => {
      if(!element.tesselate || !element.geom) {
        return (element.handles?.() || []).map(point => point.clone())
      }

      // Curves are sampled only when their geometry changes. These samples
      // establish one continuous footprint for the whole sketch.
      const vertices = element.tesselate()
      const count = Math.min(16, vertices.length)
      return Array.from({ length: count }, (_unused, index) => {
        const point = vertices[Math.round(index * (vertices.length - 1) / (count - 1))]
        return new THREE.Vector2(point[0], point[1])
      })
    }).filter(point => Number.isFinite(point.x) && Number.isFinite(point.y))

    return points
  }

  updateMask() {
    if(!this.grid) return
    const elements = this.sketchElements()
    const signature = this.geometrySignature(elements)
    if(signature != this.maskSignature) {
      const points = this.sampleSketch(elements)
      this.maskHull = convexHull(points)
      this.maskSignature = signature
    }
    const uniforms = this.grid.material.uniforms
    uniforms.maskHullCount.value = this.maskHull.length
    this.maskHull.forEach((point, index) => uniforms.maskHull.value[index].copy(point))
  }

  update(camera) {
    const relPos = camera.position.clone().sub(this.position)
    const normal = this.cachedNormal.set(0, 0, 1).applyEuler(this.rotation)
    const dist = Math.abs(normal.dot(relPos))
    const dot = Math.abs(normal.dot(relPos.normalize()))
    const size = Math.pow(10, String(Math.round(10 * dist / 4)).length) / 10
    if(!this.grid) {
      this.grid = new THREE.Mesh(
        new THREE.PlaneGeometry(VIRTUAL_GRID_SIZE, VIRTUAL_GRID_SIZE),
        gridMaterial(size),
      )
      this.add(this.grid)
      this.lastSize = size
    } else if(size != this.lastSize && dot > 0.5) {
      const uniforms = this.grid.material.uniforms
      uniforms.gridStep.value = size / 10
      uniforms.maskInnerRadius.value = size * 0.2
      uniforms.maskOuterRadius.value = size * 0.7
      this.lastSize = size
    }
    this.updateMask()

    // Move grid towards camera to avoid z-fighting
    const lookVec = camera.getWorldDirection(this.cachedVec)
    const facing = normal.dot(lookVec)
    this.grid.position.z = -0.004 * dist * facing
  }
}
