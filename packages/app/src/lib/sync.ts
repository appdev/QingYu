import type { SyncRunRequest, SyncRunResult } from "./sync-config";
import { getAppRuntime } from "../runtime";

type ApplicationSyncDependencies = {
  sync?: (input: SyncRunRequest) => Promise<SyncRunResult>;
};

export function runApplicationSync(
  request: SyncRunRequest,
  { sync = (input) => getAppRuntime().syncConfig.sync(input) }: ApplicationSyncDependencies = {}
) {
  return sync(request);
}
