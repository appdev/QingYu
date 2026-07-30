/**
 * Read-only Phase 1 seam for the future Kernel transport cutover.
 *
 * This deliberately is not an AppRuntime implementation: native shells keep
 * their current composition until Phase 2. Field-by-field mapping prevents a
 * host path, credential, or transport detail accidentally entering the shared
 * application boundary if a host adapter grows extra private fields.
 */

export type KernelCompatibilityCapabilities = {
  documents: boolean;
  history: boolean;
  portableSettings: boolean;
  resources: boolean;
  s3: boolean;
  search: boolean;
  settings: boolean;
  sync: boolean;
  webdav: boolean;
};

export type KernelCompatibilityRuntimeState = {
  capabilities: KernelCompatibilityCapabilities;
  instanceId: string;
  profile: "desktop" | "mobile" | "server";
  startupState: string;
};

export type KernelCompatibilityWorkspace = {
  displayName: string;
  generation: string;
  id: string;
  readiness: string;
  revision: string;
};

type KernelRuntimeStateSource = KernelCompatibilityRuntimeState & Record<string, unknown>;
type KernelWorkspaceSource = KernelCompatibilityWorkspace & Record<string, unknown>;

export type KernelCompatibilityClient = {
  system: {
    runtime: () => Promise<KernelRuntimeStateSource>;
  };
  workspace: {
    get: () => Promise<KernelWorkspaceSource>;
  };
};

export type KernelCompatibilityRuntime = {
  getRuntimeState: () => Promise<KernelCompatibilityRuntimeState>;
  getWorkspace: () => Promise<KernelCompatibilityWorkspace>;
};

function mapCapabilities(
  capabilities: KernelCompatibilityCapabilities,
): KernelCompatibilityCapabilities {
  return {
    documents: capabilities.documents,
    history: capabilities.history,
    portableSettings: capabilities.portableSettings,
    resources: capabilities.resources,
    s3: capabilities.s3,
    search: capabilities.search,
    settings: capabilities.settings,
    sync: capabilities.sync,
    webdav: capabilities.webdav,
  };
}

export function createKernelCompatibilityRuntime(
  client: KernelCompatibilityClient,
): KernelCompatibilityRuntime {
  return {
    getRuntimeState: async () => {
      const state = await client.system.runtime();
      return {
        capabilities: mapCapabilities(state.capabilities),
        instanceId: state.instanceId,
        profile: state.profile,
        startupState: state.startupState,
      };
    },
    getWorkspace: async () => {
      const workspace = await client.workspace.get();
      return {
        displayName: workspace.displayName,
        generation: workspace.generation,
        id: workspace.id,
        readiness: workspace.readiness,
        revision: workspace.revision,
      };
    },
  };
}
