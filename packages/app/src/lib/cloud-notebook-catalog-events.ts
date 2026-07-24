import { getAppRuntime } from "../runtime";

export const primaryCloudNotebookCatalogRequestedEvent =
  "qingyu://cloud-notebook-catalog-requested";

export function requestPrimaryCloudNotebookCatalog() {
  return getAppRuntime().window.requestPrimaryCloudNotebookCatalog();
}

export async function listenPrimaryCloudNotebookCatalogRequested(
  onRequested: () => unknown | Promise<unknown>
) {
  const runtime = getAppRuntime();
  if (!runtime.events.isAvailable()) return () => undefined;

  return runtime.events.listen<unknown>(
    primaryCloudNotebookCatalogRequestedEvent,
    (event) => {
      if (event.payload !== null) return;

      return onRequested();
    }
  );
}
