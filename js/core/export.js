import JSZip from 'jszip'
import * as THREE from 'three'

import { saveFile } from '../utils.js'
import { relativeComponentTransform } from './assembly.js'
import { exploreShape, vecFromOc } from './utils.js'


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

    // Export from an unmeshed geometry copy so these settings do not replace
    // the triangulation cached for viewport rendering.
    const copy = new window.oc.oc.BRepBuilderAPI_Copy_2(
      occurrence.compound.geom(),
      true,
      false,
    )
    const shape = copy.Shape()
    const mesher = new window.oc.oc.BRepMesh_IncrementalMesh_2(
      shape,
      maxDistance,
      false,
      maxAngle,
      false,
    )
    const transform = relativeComponentTransform(occurrence, component)
    exploreShape(shape, 'face', face => {
      appendFaceTriangles(face, transform, triangles)
      face.delete()
    })
    mesher.delete()
    shape.delete()
    copy.delete()
  })
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
const header3mf = `
<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml" />
 <Default Extension="png" ContentType="image/png" />
</Types>
`
const rels3mf = `
<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel-1" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" />
 <Relationship Target="/Metadata/thumbnail.png" Id="rel-2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail" />
</Relationships>
`

export async function export3mf(component, path) {
  var zip = new JSZip()
  zip.file('[Content_Types].xml', header3mf)
  const Metadata = zip.folder('Metadata')
  // Metadata.file('thumbnail.png', imgData, {base64: true})
  const threeD = zip.folder('3D')
  threeD.file('3dmodel.model', component.real.export_3mf())
  var _rels = zip.folder('_rels')
  _rels.file('.rels', rels3mf)
  const binarystring = await zip.generateAsync({type:'uint8array'})
  return await saveFile(binarystring, '3mf', path, component.title)
}
