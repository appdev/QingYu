export function remoteNotebookDisabledReasonKey(reason: string) {
  return reason === "notebook-name-unavailable"
    ? "notebooks.remote.invalidName"
    : "notebooks.remote.unavailable";
}
