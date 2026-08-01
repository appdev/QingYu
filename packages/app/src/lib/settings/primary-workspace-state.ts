import { normalizeNullableString } from "@markra/shared";
import { getAppRuntime } from "../../runtime";

export type PrimaryWorkspaceState = {
  desktopWorkspaceRoot: string | null;
  desktopPath: string | null;
  managedName: string | null;
  onboardingCompleted: boolean;
  onboardingRequestedForNextLaunch?: true;
  version: 3;
};

export const defaultPrimaryWorkspaceState: PrimaryWorkspaceState = {
  desktopWorkspaceRoot: null,
  desktopPath: null,
  managedName: null,
  onboardingCompleted: false,
  version: 3
};

export function isValidManagedNotebookName(name: string) {
  const normalizedName = name.toLocaleLowerCase("en-US");
  return name.length > 0 &&
    name !== "." &&
    name !== ".." &&
    !name.includes("/") &&
    !name.includes("\\") &&
    !name.includes("\0") &&
    normalizedName !== ".qingyu" &&
    normalizedName !== ".markra-sync" &&
    !normalizedName.startsWith(".markra-sync-stage-");
}

export function normalizePrimaryWorkspaceState(value: unknown): PrimaryWorkspaceState {
  if (!value || typeof value !== "object") return defaultPrimaryWorkspaceState;
  const candidate = value as Partial<PrimaryWorkspaceState>;
  if (candidate.version !== 3) return defaultPrimaryWorkspaceState;

  const hasOwn = (key: keyof PrimaryWorkspaceState) =>
    Object.prototype.hasOwnProperty.call(candidate, key);
  const nullableStringHasInvalidType = (key: keyof Pick<
    PrimaryWorkspaceState,
    "desktopWorkspaceRoot" | "desktopPath" | "managedName"
  >) => hasOwn(key) && candidate[key] !== null && typeof candidate[key] !== "string";
  if (
    nullableStringHasInvalidType("desktopWorkspaceRoot") ||
    nullableStringHasInvalidType("desktopPath") ||
    nullableStringHasInvalidType("managedName") ||
    (hasOwn("onboardingCompleted") && typeof candidate.onboardingCompleted !== "boolean") ||
    (
      hasOwn("onboardingRequestedForNextLaunch") &&
      typeof candidate.onboardingRequestedForNextLaunch !== "boolean"
    )
  ) {
    return defaultPrimaryWorkspaceState;
  }

  const desktopWorkspaceRoot = normalizeNullableString(candidate.desktopWorkspaceRoot);
  const desktopPath = normalizeNullableString(candidate.desktopPath);
  const managedName = typeof candidate.managedName === "string" ? candidate.managedName : null;
  const hasDesktopWorkspaceRoot = desktopWorkspaceRoot !== null;
  const hasDesktopPath = desktopPath !== null;
  if (
    hasDesktopWorkspaceRoot !== hasDesktopPath ||
    (hasDesktopWorkspaceRoot && managedName !== null) ||
    (managedName !== null && !isValidManagedNotebookName(managedName))
  ) {
    return defaultPrimaryWorkspaceState;
  }

  return {
    desktopWorkspaceRoot,
    desktopPath,
    managedName,
    onboardingCompleted: candidate.onboardingCompleted === true,
    ...(candidate.onboardingRequestedForNextLaunch === true
      ? { onboardingRequestedForNextLaunch: true as const }
      : {}),
    version: 3
  };
}

function nativePrimaryWorkspaceBridge() {
  const settings = getAppRuntime().settings;
  const read = settings.readPrimaryWorkspaceState;
  const write = settings.writePrimaryWorkspaceState;
  if (!read || !write) {
    throw new Error("The native primary workspace metadata bridge is unavailable.");
  }
  return { read, write };
}

export async function loadPrimaryWorkspaceState(): Promise<PrimaryWorkspaceState> {
  const settings = nativePrimaryWorkspaceBridge();
  return normalizePrimaryWorkspaceState(await settings.read());
}

export async function savePrimaryWorkspaceState(
  state: PrimaryWorkspaceState
): Promise<PrimaryWorkspaceState> {
  const settings = nativePrimaryWorkspaceBridge();
  const normalized = normalizePrimaryWorkspaceState(state);
  const result = await settings.write({ state: normalized });
  return normalizePrimaryWorkspaceState(result.state);
}

export async function saveCanonicalPrimaryWorkspaceState(
  state: PrimaryWorkspaceState,
  expectedState: PrimaryWorkspaceState
): Promise<PrimaryWorkspaceState> {
  const settings = nativePrimaryWorkspaceBridge();
  const result = await settings.write({
    expectedState: normalizePrimaryWorkspaceState(expectedState),
    state: normalizePrimaryWorkspaceState(state)
  });
  return normalizePrimaryWorkspaceState(result.state);
}

export async function updatePrimaryWorkspaceState(
  change: Partial<Omit<PrimaryWorkspaceState, "version">>
): Promise<PrimaryWorkspaceState> {
  const current = await loadPrimaryWorkspaceState();
  return savePrimaryWorkspaceState(normalizePrimaryWorkspaceState({
    ...current,
    ...change,
    version: 3
  }));
}
