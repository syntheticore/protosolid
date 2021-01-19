export function saveFile(data, filename, filetype) {
  var blob = new Blob([data], {filetype})
  saveBlob(blob, filename, filetype)
}

export function saveBlob(blob, filename, filetype) {
  filename = filename.replace(new RegExp(' ', 'g'), '_') + '.' + filetype
  const a = document.createElement("a")
  const url = URL.createObjectURL(blob)
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  setTimeout(function() {
    document.body.removeChild(a)
    window.URL.revokeObjectURL(url)
  }, 0)
}
