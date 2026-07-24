import {
  analyzeWorkspaceResourceBatch,
  isWorkspaceResourceWorkerRequest
} from "../lib/workspace-resource-worker";

self.onmessage = (event: MessageEvent<unknown>) => {
  if (!isWorkspaceResourceWorkerRequest(event.data)) return;

  analyzeWorkspaceResourceBatch(event.data).forEach((response) => self.postMessage(response));
};
