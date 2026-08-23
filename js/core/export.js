import JSZip from 'jszip'
import * as THREE from 'three'

import { saveFile } from '../utils.js'
import { relativeComponentTransform } from './assembly.js'
import { exploreShape, transformGeometry, vecFromOc } from './utils.js'


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
  const triangles = []

  // Export from an unmeshed geometry copy so these settings do not replace
  // the triangulation cached for viewport rendering.
  const copy = new window.oc.oc.BRepBuilderAPI_Copy_2(geometry, true, false)
  const shape = copy.Shape()
  const mesher = new window.oc.oc.BRepMesh_IncrementalMesh_2(
    shape,
    maxDistance,
    false,
    maxAngle,
    false,
  )
  exploreShape(shape, 'face', face => {
    appendFaceTriangles(face, transform, triangles)
    face.delete()
  })
  mesher.delete()
  shape.delete()
  copy.delete()
  return triangles
}

function appendFaceTriangles(face, componentTransform, triangles) {
  const location = new window.oc.oc.TopLoc_Location_1()
  const triangulation = window.oc.oc.BRep_Tool
    .Triangulation(face, location, 0)
    .get()
  if(!triangulation) {
    location.delete()
    return
  }

  const locationTransform = location.Transformation()
  const transform = componentTransform.clone()
    .multiply(matrixFromOcTransform(locationTransform))
  const reversed = face.Orientation_1() ==
    window.oc.oc.TopAbs_Orientation.TopAbs_REVERSED
  const vertexOrder = reversed ? [1, 3, 2] : [1, 2, 3]
  for(let i = 1; i <= triangulation.NbTriangles(); i++) {
    const triangle = triangulation.Triangle(i)
    triangles.push(vertexOrder.map(vertexIndex => {
      const point = triangulation.Node(triangle.Value(vertexIndex))
      const vertex = vecFromOc(point).applyMatrix4(transform)
      point.delete()
      return vertex
    }))
    triangle.delete()
  }
  locationTransform.delete()
  location.delete()
}

function matrixFromOcTransform(transform) {
  return new THREE.Matrix4().set(
    transform.Value(1, 1), transform.Value(1, 2), transform.Value(1, 3), transform.Value(1, 4),
    transform.Value(2, 1), transform.Value(2, 2), transform.Value(2, 3), transform.Value(2, 4),
    transform.Value(3, 1), transform.Value(3, 2), transform.Value(3, 3), transform.Value(3, 4),
    0, 0, 0, 1,
  )
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


// STEP
let stepExportId = 0

export async function exportStep(component, path) {
  const title = component.creator.title
  const occurrences = component.getChildren()
    .filter(occurrence => occurrence.compound.geom)
  if(!occurrences.length) throw new Error(`Cannot export empty component "${title}"`)

  const oc = window.oc.oc
  const writer = new oc.STEPControl_Writer_1()
  const compound = new oc.TopoDS_Compound()
  const builder = new oc.BRep_Builder()
  const tempPath = `/protosolid-export-${++stepExportId}.step`

  try {
    builder.MakeCompound(compound)
    occurrences.forEach(occurrence => {
      const transform = relativeComponentTransform(occurrence, component)
      const transformer = transformGeometry(occurrence.compound.geom(), transform)
      try {
        const shape = transformer.Shape()
        try {
          builder.Add(compound, shape)
        } finally {
          shape.delete()
        }
      } finally {
        transformer.delete()
      }
    })

    const progress = new oc.Message_ProgressRange_1()
    let transferStatus
    try {
      transferStatus = writer.Transfer(
        compound,
        oc.STEPControl_StepModelType.STEPControl_AsIs,
        true,
        progress,
      )
    } finally {
      progress.delete()
    }
    if(transferStatus !== oc.IFSelect_ReturnStatus.IFSelect_RetDone) {
      throw new Error(`OpenCascade could not translate "${title}" to STEP`)
    }

    const writeStatus = writer.Write(tempPath)
    if(writeStatus !== oc.IFSelect_ReturnStatus.IFSelect_RetDone) {
      throw new Error(`OpenCascade could not write STEP for "${title}"`)
    }
    const bytes = oc.FS.readFile(tempPath, { encoding: 'binary' })
    return await saveFile(bytes, 'step', path, title)
  } finally {
    try { oc.FS.unlink(tempPath) } catch(_) {}
    builder.delete()
    compound.delete()
    writer.delete()
  }
}
