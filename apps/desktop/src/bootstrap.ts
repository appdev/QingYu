export type ApplicationBootstrapOptions<Runtime> = {
  configureRuntime: (runtime: Runtime) => unknown;
  loadRuntime: () => Promise<Runtime>;
  reload: () => unknown;
  renderApp: () => unknown;
  renderError: (onRetry: () => unknown) => unknown;
};

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
