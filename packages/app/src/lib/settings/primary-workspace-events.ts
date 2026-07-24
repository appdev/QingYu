import { getAppRuntime } from "../../runtime";

const primaryWorkspaceChangedEvent = "qingyu://primary-workspace-changed";

export type PrimaryWorkspaceChangedPayload = {
  generation: number;
  sourceId: string;
};

function isPrimaryWorkspaceChangedPayload(value: unknown): value is PrimaryWorkspaceChangedPayload {
  if (typeof value !== "object" || value === null) return false;
  const payload = value as Partial<PrimaryWorkspaceChangedPayload>;

  return Number.isInteger(payload.generation) &&
    Number(payload.generation) >= 0 &&
    typeof payload.sourceId === "string" &&
    payload.sourceId.length > 0;
}

export async function notifyPrimaryWorkspaceChanged(payload: PrimaryWorkspaceChangedPayload) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(primaryWorkspaceChangedEvent, payload);
}

export async function listenPrimaryWorkspaceChanged(
  sourceId: string,
  onChanged: (payload: PrimaryWorkspaceChangedPayload) => unknown
) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<PrimaryWorkspaceChangedPayload>(
    primaryWorkspaceChangedEvent,
    (event) => {
      if (!isPrimaryWorkspaceChangedPayload(event.payload)) return;
      if (event.payload.sourceId === sourceId) return;
      onChanged(event.payload);
    }
  );
}
