export function saveFile(data, filename, filetype) {
  var file = new Blob([data], {filetype});
  const a = document.createElement("a")
  const url = URL.createObjectURL(file);
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  setTimeout(function() {
    document.body.removeChild(a);
    window.URL.revokeObjectURL(url);
  }, 0);
}
