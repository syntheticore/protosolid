import JSZip from 'jszip'

import { saveFile } from '../utils.js'


// STL
export async function exportStl(component, path) {
  const stl = component.real.export_stl(component.title)
  return await saveFile(stl, 'stl', path, component.title)
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

export function export3mf(component, path) {
  var zip = new JSZip()
  zip.file('[Content_Types].xml', header3mf)
  const Metadata = zip.folder('Metadata')
  // Metadata.file('thumbnail.png', imgData, {base64: true})
  const threeD = zip.folder('3D')
  threeD.file('3dmodel.model', component.real.export_3mf())
  var _rels = zip.folder('_rels')
  _rels.file('.rels', rels3mf)
  zip.generateAsync({type:'uint8array'}).then(function(binarystring) {
    saveFile(binarystring, '3mf', path, component.title)
  })
}
