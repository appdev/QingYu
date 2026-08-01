export type RestoreCandidateSummary = {
  recoverableDraftCount: number;
  validFileCount: number;
  workspaceReady: boolean;
};

export function workspaceSurfaceForRestore(
  summary: RestoreCandidateSummary,
): "editor" | "home" | "recovery" {
  if (!summary.workspaceReady) {
    return "recovery";
  }

  if (summary.validFileCount > 0 || summary.recoverableDraftCount > 0) {
    return "editor";
  }

  return "home";
}
