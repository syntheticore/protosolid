import JSZip from 'jszip'
import * as THREE from 'three'

import { saveFile } from '../utils.js'
import { relativeComponentTransform } from './assembly.js'
import { shapex, tessellateMesh, transformFromMatrix } from './shapex.js'
import { exportGcode } from './cam.js'


// STEP
// Exact bodies of the component and its subcomponents in the component's
// frame. An assembly writes one product per component definition, placed by
// its occurrences. A flat assembly places every component's product in the
// exported component directly, and a single part holds all bodies.
export async function exportStep(component, path, config = {}) {
  const title = component.creator.title
  const owned = []
  const products = []
  const product = (name, shape, occurrences) => shape || occurrences.length ? products.push({
    name, bodies: shape?.bodies || null, colors: shape?.colors || [], occurrences,
  }) - 1 : null

  // A component's own exported solids and their colours.
  const own = occurrence => {
    const solids = occurrence.compound.solids()
    const exported = solids.filter(solid => config.includeHidden || !occurrence.isItemHidden(solid.id))
    if(!exported.length) return null
    let bodies = occurrence.compound.geom()
    if(exported.length < solids.length) {
      const parts = exported.map(solid => solid.geom().toCompound())
      bodies = parts[0].combined(parts.slice(1))
      owned.push(...parts, bodies)
    }
    const color = config.colors === false ? null : stepColor(occurrence.creator.color)
    return { bodies, colors: exported.map(() => color) }
  }
  const children = occurrence => occurrence.children.filter(child => config.includeHidden || !child.creator.hidden)
  const descendants = occurrence => children(occurrence).flatMap(child => [child, ...descendants(child)])
  const placement = occurrence => transformFromMatrix(relativeComponentTransform(occurrence, component))

  try {
    let root
    if(config.structure == 'single') {
      const shapes = [component, ...descendants(component)].flatMap(occurrence => {
        const shape = own(occurrence)
        if(!shape) return []
        const bodies = shape.bodies.transformed(placement(occurrence))
        owned.push(bodies)
        return [{ ...shape, bodies }]
      })
      const bodies = shapes[0]?.bodies.combined(shapes.slice(1).map(shape => shape.bodies))
      if(bodies) owned.push(bodies)
      root = product(title, bodies && { bodies, colors: shapes.flatMap(shape => shape.colors) }, [])
    } else if(config.structure == 'flat') {
      const indices = new Map()
      const occurrences = descendants(component).map(occurrence => {
        const id = occurrence.sourceId()
        if(!indices.has(id)) indices.set(id, product(occurrence.creator.title, own(occurrence), []))
        return { product: indices.get(id), placement: placement(occurrence) }
      }).filter(occurrence => occurrence.product !== null)
      root = product(title, own(component), occurrences)
    } else {
      // Occurrences hiding different children are different products.
      const signature = occurrence => `${occurrence.sourceId()}(${children(occurrence).map(signature)})`
      const indices = new Map()
      const assembly = occurrence => {
        const id = signature(occurrence)
        if(!indices.has(id)) {
          const occurrences = children(occurrence)
            .map(child => ({ product: assembly(child), placement: transformFromMatrix(child.localTransform()) }))
            .filter(child => child.product !== null)
          indices.set(id, product(occurrence.creator.title, own(occurrence), occurrences))
        }
        return indices.get(id)
      }
      root = assembly(component)
    }
    if(root === null) throw new Error(`Cannot export empty component "${title}"`)
    const step = shapex.StepAssembly.toStep({ products, roots: [root], unit: config.unit || 'mm' })
    return await saveFile(step, 'step', path, title)
  } finally {
    owned.forEach(compound => compound.free())
  }
}

// STEP colours are sRGB components in [0, 1].
function stepColor(color) {
  const { r, g, b } = new THREE.Color(color).getRGB({}, THREE.SRGBColorSpace)
  return [r, g, b]
}


// STL
export async function exportStl(component, path, config = {}) {
  const title = component.creator.title
  const stl = binaryStl(component, config, title)
  return await saveFile(stl, 'stl', path, title)
}

function binaryStl(component, config, title) {
  const maxDistance = positiveNumber(config.maxDistance, 0.1)
  const maxAngle = THREE.MathUtils.degToRad(positiveNumber(config.maxAngle, 10.0))
  const triangles = componentTriangles(component, maxDistance, maxAngle)
  if(!triangles.length) throw new Error(`Cannot export empty component "${title}"`)

  const buffer = new ArrayBuffer(84 + triangles.length * 50)
  const bytes = new Uint8Array(buffer)
  const view = new DataView(buffer)
  const header = new TextEncoder().encode(`ProtoSolid: ${title}`)
  bytes.set(header.subarray(0, 80))
  view.setUint32(80, triangles.length, true)

  let offset = 84
  const edge1 = new THREE.Vector3()
  const edge2 = new THREE.Vector3()
  const normal = new THREE.Vector3()
  triangles.forEach(vertices => {
    edge1.subVectors(vertices[1], vertices[0])
    edge2.subVectors(vertices[2], vertices[0])
    normal.crossVectors(edge1, edge2).normalize()
    writeVector(view, offset, normal)
    offset += 12
    vertices.forEach(vertex => {
      writeVector(view, offset, vertex)
      offset += 12
    })
    view.setUint16(offset, 0, true)
    offset += 2
  })
  return bytes
}

export function componentTriangles(component, maxDistance, maxAngle) {
  const triangles = []
  component.getChildren().forEach(occurrence => {
    if(!occurrence.compound.geom) return
    triangles.push(...geometryTriangles(
      occurrence.compound.geom(),
      relativeComponentTransform(occurrence, component),
      maxDistance,
      maxAngle,
    ))
  })
  return triangles
}

function componentTriangleMeshes(component, maxDistance, maxAngle) {
  const meshes = []
  component.getChildren().forEach(occurrence => {
    const transform = relativeComponentTransform(occurrence, component)
    occurrence.compound.solids().forEach(solid => {
      const triangles = geometryTriangles(
        solid.geom(),
        transform,
        maxDistance,
        maxAngle,
      )
      if(triangles.length) meshes.push(triangles)
    })
  })
  return meshes
}

function geometryTriangles(geometry, transform, maxDistance, maxAngle) {
  const mesh = tessellateMesh(geometry, {
    chordTolerance: maxDistance,
    angularTolerance: maxAngle,
  })
  const positions = mesh.positions
  const triangles = []
  for(let offset = 0; offset < positions.length; offset += 9) {
    triangles.push([0, 3, 6].map(index => new THREE.Vector3(
      positions[offset + index],
      positions[offset + index + 1],
      positions[offset + index + 2],
    ).applyMatrix4(transform)))
  }
  return triangles
}


function writeVector(view, offset, vector) {
  view.setFloat32(offset, vector.x, true)
  view.setFloat32(offset + 4, vector.y, true)
  view.setFloat32(offset + 8, vector.z, true)
}

function positiveNumber(value, fallback) {
  value = Number(value)
  return Number.isFinite(value) && value > 0 ? value : fallback
}

export async function exportConfigured(component, config) {
  const exporter = {
    STL: exportStl,
    '3MF': export3mf,
    STEP: exportStep,
  }[config.format]
  if(!exporter) throw new Error(`Unsupported export format: ${config.format}`)
  return await exporter(component, config.path, config)
}

export async function autoExportComponents(root) {
  const exportedConfigs = new Set()
  for(const component of root.getChildren()) {
    for(const config of component.creator.exportConfigs || []) {
      if(!config.autoSave || exportedConfigs.has(config)) continue
      exportedConfigs.add(config)
      if(!config.path) {
        throw new Error(`Auto-export "${config.title}" has no destination path`)
      }
      await exportConfigured(component, config)
    }
    for(const setup of component.creator.camSetups) {
      if(!setup.autoSave || exportedConfigs.has(setup)) continue
      exportedConfigs.add(setup)
      if(!setup.path) {
        throw new Error(`Auto-export "${setup.title}" has no destination path`)
      }
      await exportGcode(setup, component)
    }
  }
}


// 3MF
const header3mf = `<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml" />
</Types>
`
const rels3mf = `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel-1" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" />
</Relationships>
`

export async function export3mf(component, path, config = {}) {
  const title = component.creator.title
  const maxDistance = positiveNumber(config.maxDistance, 0.1)
  const maxAngle = THREE.MathUtils.degToRad(positiveNumber(config.maxAngle, 10.0))
  const meshes = componentTriangleMeshes(component, maxDistance, maxAngle)
  if(!meshes.length) throw new Error(`Cannot export empty component "${title}"`)

  const zip = new JSZip()
  zip.file('[Content_Types].xml', header3mf)
  const threeD = zip.folder('3D')
  threeD.file('3dmodel.model', model3mf(title, meshes))
  const _rels = zip.folder('_rels')
  _rels.file('.rels', rels3mf)
  const bytes = await zip.generateAsync({
    type: 'uint8array',
    compression: 'DEFLATE',
    compressionOptions: { level: 6 },
  })
  return await saveFile(bytes, '3mf', path, title)
}

function model3mf(title, meshes) {
  const objects = meshes.map((triangles, index) => {
    const { vertices, faces } = indexedMesh(triangles)
    return `  <object id="${index + 1}" type="model" name="${escapeXml(title)}">
   <mesh>
    <vertices>
${vertices.map(vertex =>
    `     <vertex x="${vertex.x}" y="${vertex.y}" z="${vertex.z}" />`,
  ).join('\n')}
    </vertices>
    <triangles>
${faces.map(face =>
    `     <triangle v1="${face[0]}" v2="${face[1]}" v3="${face[2]}" />`,
  ).join('\n')}
    </triangles>
   </mesh>
  </object>`
  })
  const items = meshes.map((_, index) =>
    `  <item objectid="${index + 1}" printable="1" />`,
  )

  return `<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <metadata name="Application">ProtoSolid</metadata>
 <metadata name="Title">${escapeXml(title)}</metadata>
 <resources>
${objects.join('\n')}
 </resources>
 <build>
${items.join('\n')}
 </build>
</model>
`
}

function indexedMesh(triangles) {
  const vertices = []
  const indices = new Map()
  const faces = triangles.map(triangle => triangle.map(vertex => {
    const x = vertex.x === 0 ? 0 : vertex.x
    const y = vertex.y === 0 ? 0 : vertex.y
    const z = vertex.z === 0 ? 0 : vertex.z
    const key = `${x},${y},${z}`
    let index = indices.get(key)
    if(index === undefined) {
      index = vertices.length
      indices.set(key, index)
      vertices.push({ x, y, z })
    }
    return index
  }))
  return { vertices, faces }
}

function escapeXml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;')
}
