export function shouldFocusEditorOnReady(markdown = "") {
  const params = new URLSearchParams(window.location.search);
  return params.has("blank") || params.has("path") || markdown.trim() === "";
}
