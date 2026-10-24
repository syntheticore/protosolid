// Follow the constraint manifold from the last accepted geometry. Pointer
// coordinates are objectives, never an initial guess written into the sketch.
const tolerance = 1e-7
const minimumSize = 1e-5

function index(primitives) {
  return new Map(primitives.map(primitive => [primitive.id, primitive]))
}

function sizes(primitives) {
  const byId = index(primitives)
  return primitives.flatMap(primitive => {
    if(primitive.type == 'circle') return [primitive.radius]
    if(primitive.type == 'arc') {
      const a = byId.get(primitive.start_id), b = byId.get(primitive.end_id)
      return [primitive.radius, Math.hypot(a.x - b.x, a.y - b.y)]
    }
    if(primitive.type != 'line') return []
    const a = byId.get(primitive.p1_id), b = byId.get(primitive.p2_id)
    return [Math.hypot(a.x - b.x, a.y - b.y)]
  })
}

export function validGeometry(primitives) {
  const byId = index(primitives)
  return primitives.every(primitive => {
    if(primitive.type == 'point') return Number.isFinite(primitive.x) && Number.isFinite(primitive.y)
    if(primitive.type == 'circle' || primitive.type == 'arc') {
      if(!Number.isFinite(primitive.radius) || primitive.radius <= minimumSize) return false
      if(primitive.type == 'arc') {
        const center = byId.get(primitive.c_id)
        return ['start', 'end'].every(end => {
          const point = byId.get(primitive[`${end}_id`])
          const angle = primitive[`${end}_angle`]
          return Number.isFinite(angle) && Math.hypot(
            point.x - center.x - primitive.radius * Math.cos(angle),
            point.y - center.y - primitive.radius * Math.sin(angle),
          ) <= tolerance * Math.max(1, primitive.radius)
        })
      }
    }
    return true
  })
}

// Check the geometric equations independently of the solver's convergence
// status: at a singularity a small numerical step need not be a solution.
export function satisfiesConstraints(primitives, constraints) {
  const byId = index(primitives)
  const point = id => byId.get(id)
  const vector = (a, b) => [b.x - a.x, b.y - a.y]
  const norm = v => Math.hypot(...v)
  const direction = id => {
    const line = byId.get(id)
    return vector(point(line.p1_id), point(line.p2_id))
  }
  const distance = (a, b) => norm(vector(point(a), point(b)))
  const cross = (a, b) => a[0] * b[1] - a[1] * b[0]
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1]
  const lineDistance = (p, l) => {
    const line = byId.get(l), dir = direction(l)
    return Math.abs(cross(vector(point(line.p1_id), point(p)), dir)) / norm(dir)
  }
  const near = (a, b=0) => Number.isFinite(a) && Math.abs(a - b) <= tolerance * Math.max(1, Math.abs(b))
  return constraints.every(c => {
    if(c.type == 'free_param' || c.temporary) return true
    switch(c.type) {
      case 'p2p_coincident': return near(distance(c.p1_id, c.p2_id))
      case 'horizontal_pp': return near(point(c.p1_id).y - point(c.p2_id).y)
      case 'vertical_pp': return near(point(c.p1_id).x - point(c.p2_id).x)
      case 'coordinate_x': return near(point(c.p_id).x, c.x)
      case 'coordinate_y': return near(point(c.p_id).y, c.y)
      case 'p2p_distance': return near(distance(c.p1_id, c.p2_id), c.distance)
      case 'point_on_line_pl': return near(lineDistance(c.p_id, c.l_id))
      case 'p2l_distance': return near(lineDistance(c.p_id, c.l_id), Math.abs(c.distance))
      case 'point_on_perp_bisector_pl': {
        const line = byId.get(c.l_id)
        return near(distance(c.p_id, line.p1_id), distance(c.p_id, line.p2_id))
      }
      case 'parallel':
      case 'perpendicular_ll':
      case 'l2l_angle_ll': {
        const a = direction(c.l1_id), b = direction(c.l2_id)
        const product = norm(a) * norm(b)
        if(product <= minimumSize ** 2) return false
        if(c.type == 'parallel') return near(cross(a, b) / product)
        if(c.type == 'perpendicular_ll') return near(dot(a, b) / product)
        const error = Math.atan2(cross(a, b), dot(a, b)) - c.angle
        return near(Math.atan2(Math.sin(error), Math.cos(error)))
      }
      case 'equal_length': return near(norm(direction(c.l1_id)), norm(direction(c.l2_id)))
      case 'point_on_circle':
      case 'point_on_arc': {
        const circle = byId.get(c.c_id || c.a_id)
        return near(distance(c.p_id, circle.c_id), circle.radius)
      }
      case 'circle_radius':
      case 'arc_radius': return near(byId.get(c.c_id || c.a_id).radius, c.radius)
      case 'circle_diameter': return near(byId.get(c.c_id).radius * 2, c.diameter)
      case 'equal_radius_cc': return near(byId.get(c.c1_id).radius, byId.get(c.c2_id).radius)
      case 'equal_radius_ca': return near(byId.get(c.c1_id).radius, byId.get(c.a2_id).radius)
      case 'equal_radius_aa': return near(byId.get(c.a1_id).radius, byId.get(c.a2_id).radius)
      case 'difference': return near(byId.get(c.param1.o_id)[c.param1.prop] - byId.get(c.param2.o_id)[c.param2.prop], c.difference)
      case 'tangent_lc':
      case 'tangent_la': {
        const circle = byId.get(c.c_id || c.a_id)
        return near(lineDistance(circle.c_id, c.l_id), circle.radius)
      }
      case 'tangent_cc':
      case 'tangent_ca':
      case 'tangent_aa': {
        const a = byId.get(c.c1_id || c.c_id || c.a1_id), b = byId.get(c.c2_id || c.a_id || c.a2_id)
        const d = distance(a.c_id, b.c_id)
        return near(d, a.radius + b.radius) || near(d, Math.abs(a.radius - b.radius))
      }
      // Arc rules are checked by validGeometry. Spline contacts are checked
      // against the exact application curve after its control points update.
      case 'arc_rules':
      case 'point_on_bspline': return true
      default: throw new Error(`Unsupported sketch drag equation: ${c.type}`)
    }
  })
}

function continuous(before, after, distance, checkSizes=true) {
  const byId = index(after)
  const movementLimit = Math.max(distance * 4, tolerance)
  if(before.some(primitive => {
    const next = byId.get(primitive.id)
    if(primitive.type == 'point') return Math.hypot(next.x - primitive.x, next.y - primitive.y) >
      (primitive.fixed ? tolerance : movementLimit)
    if(primitive.type == 'circle' || primitive.type == 'arc') return Math.abs(next.radius - primitive.radius) > movementLimit
    return false
  })) return false
  if(!checkSizes) return true
  const oldSizes = sizes(before), newSizes = sizes(after)
  return oldSizes.every((size, i) => size <= minimumSize ||
    (newSizes[i] > minimumSize && newSizes[i] >= size / 2 && newSizes[i] <= size * 2))
}

export function solveDrag(primitives, constraints, targets, solve) {
  let accepted = primitives.map(primitive => ({ ...primitive }))
  const initialSizes = sizes(primitives).filter(size => size > minimumSize)
  const scale = initialSizes.length ? Math.min(...initialSizes) : 1
  let step = Math.max(scale / 8, minimumSize)
  const targetDistance = state => {
    const byId = index(state)
    return Math.hypot(...targets.map(target => target.value - byId.get(target.id)[target.property]))
  }
  const initialDistance = targetDistance(accepted)
  if(!Number.isFinite(initialDistance)) return { results: accepted, failed: true }
  const targetGroups = [...new Set(targets.map(target => target.property))]
    .map(property => targets.filter(target => target.property == property))
  // Bounded work per pointer event. A new event resumes from the accepted
  // geometry, including after reversing away from a singularity.
  for(let attempt = 0; attempt < 48; attempt++) {
    const remaining = targetDistance(accepted)
    if(remaining <= tolerance) break
    const distance = Math.min(step, remaining)
    const byId = index(accepted)
    const objectivesFor = group => {
      const remaining = Math.hypot(...group.map(target => target.value - byId.get(target.id)[target.property]))
      const fraction = Math.min(1, step / remaining)
      return group.map((target, i) => {
        const value = byId.get(target.id)[target.property]
        const desired = value + (target.value - value) * fraction
        return target.property == 'radius' ? {
          id: `drag-${i}`, type: target.type == 'arc' ? 'arc_radius' : 'circle_radius',
          [target.type == 'arc' ? 'a_id' : 'c_id']: target.id, radius: desired, temporary: true,
        } : {
          id: `drag-${i}`, type: `coordinate_${target.property}`, p_id: target.id,
          [target.property]: desired, temporary: true,
        }
      })
    }
    const solveCandidate = objectives => {
      // The wrapper mutates its primitive objects when applying a solution.
      const solution = solve([...accepted.map(p => ({ ...p })), ...constraints, ...objectives])
      const candidateById = index(solution.results)
      const candidate = accepted.map(p => candidateById.get(p.id))
      if(solution.failed || solution.conflicting || candidate.some(p => !p) ||
        !validGeometry(candidate) ||
        !continuous(accepted, candidate, distance) ||
        !continuous(primitives, candidate, initialDistance, false) ||
        !satisfiesConstraints(candidate, constraints)) return null
      return candidate
    }
    let candidate = solveCandidate(objectivesFor(targets))
    if(targetGroups.length > 1 && (!candidate || targetDistance(candidate) > remaining - distance + tolerance)) {
      // A locked coordinate can make the combined pointer objectives conflict.
      // Minimize along each coordinate direction and keep the closest valid
      // step, so an unreachable direction does not block the remaining freedom.
      for(const group of targetGroups) {
        const trial = solveCandidate(objectivesFor(group))
        if(trial && (!candidate || targetDistance(trial) < targetDistance(candidate))) candidate = trial
      }
    }
    if(!candidate) {
      step /= 2
      if(step < minimumSize / 8) break
      continue
    }
    const candidateById = index(candidate)
    const movement = Math.hypot(...targets.map(target =>
      candidateById.get(target.id)[target.property] - byId.get(target.id)[target.property]
    ))
    if(movement <= tolerance || targetDistance(candidate) >= remaining) break
    accepted = candidate
    const localSizes = sizes(accepted).filter(size => size > minimumSize)
    const localScale = localSizes.length ? Math.min(...localSizes) : scale
    step = Math.min(step * 2, Math.max(localScale / 2, minimumSize))
  }
  return { results: accepted, failed: !satisfiesConstraints(accepted, constraints) }
}
