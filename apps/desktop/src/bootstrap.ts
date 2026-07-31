export type ApplicationBootstrapOptions<Runtime> = {
  configureRuntime: (runtime: Runtime) => unknown;
  loadRuntime: () => Promise<Runtime>;
  reload: () => unknown;
  renderApp: () => unknown;
  renderError: (onRetry: () => unknown) => unknown;
};

export interface ApplicationMountOwner {
  start(): Promise<unknown>;
  close(): unknown;
}

export interface ApplicationMountBootstrapOptions {
  readonly mountOwner: ApplicationMountOwner;
  readonly reload: () => unknown;
  readonly renderError: (onRetry: () => unknown) => unknown;
}

export async function bootstrapApplication<Runtime>({
  configureRuntime,
  loadRuntime,
  reload,
  renderApp,
  renderError
}: ApplicationBootstrapOptions<Runtime>) {
  try {
    const runtime = await loadRuntime();
    configureRuntime(runtime);
    renderApp();
  } catch {
    renderError(reload);
  }
}

export async function bootstrapApplicationMount(
  options: ApplicationMountBootstrapOptions
): Promise<() => undefined> {
  let active = true;
  const stop = () => {
    if (!active) return undefined;
    active = false;
    options.mountOwner.close();
    return undefined;
  };
  try {
    await options.mountOwner.start();
  } catch {
    stop();
    options.renderError(options.reload);
  }
  return stop;
}
