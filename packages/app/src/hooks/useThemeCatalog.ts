import { diagnosticErrorMessage } from "@markra/shared";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  mergeThemeCatalog,
  type MergedThemeCatalog,
  type ThemeDescriptor,
  type ThemeImportResult
} from "../lib/themes/theme-catalog";
import {
  listenThemeCatalogChanged,
  notifyThemeCatalogChanged
} from "../lib/settings/settings-events";
import { getAppRuntime } from "../runtime";

const emptyCatalog = mergeThemeCatalog({ invalidFiles: [], themes: [] });

export function useThemeCatalog() {
  const [catalog, setCatalog] = useState<MergedThemeCatalog>(emptyCatalog);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const requestTokenRef = useRef(0);
  const mountedRef = useRef(true);

  const refresh = useCallback(async () => {
    const token = ++requestTokenRef.current;
    setLoading(true);
    try {
      const snapshot = await getAppRuntime().themes.list();
      if (!mountedRef.current || token !== requestTokenRef.current) return;
      setCatalog(mergeThemeCatalog(snapshot));
      setError(null);
    } catch (refreshError) {
      if (!mountedRef.current || token !== requestTokenRef.current) return;
      setError(diagnosticErrorMessage(refreshError));
    } finally {
      if (mountedRef.current && token === requestTokenRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    refresh().catch(() => {});
    let cleanup: (() => unknown) | null = null;
    listenThemeCatalogChanged(() => {
      refresh().catch(() => {});
    }).then((stopListening) => {
      if (!mountedRef.current) {
        stopListening();
        return;
      }
      cleanup = stopListening;
    }).catch(() => {});

    return () => {
      mountedRef.current = false;
      requestTokenRef.current += 1;
      cleanup?.();
    };
  }, [refresh]);

  const mutateAndRefresh = useCallback(async <T,>(operation: () => Promise<T>) => {
    const result = await operation();
    await notifyThemeCatalogChanged();
    await refresh();
    return result;
  }, [refresh]);

  const importTheme = useCallback(async (): Promise<ThemeImportResult | null> => {
    const result = await getAppRuntime().themes.importFile();
    if (result?.kind === "imported") {
      await notifyThemeCatalogChanged();
      await refresh();
    }
    return result;
  }, [refresh]);

  const replaceTheme = useCallback((sourcePath: string, expectedFingerprint: string) => (
    mutateAndRefresh(() => getAppRuntime().themes.replaceFile(sourcePath, expectedFingerprint))
  ), [mutateAndRefresh]);

  const deleteTheme = useCallback((theme: ThemeDescriptor) => (
    mutateAndRefresh(() => getAppRuntime().themes.delete(theme.id, theme.fingerprint))
  ), [mutateAndRefresh]);

  const actions = useMemo(() => ({
    deleteTheme,
    importTheme,
    openDirectory: getAppRuntime().themes.openDirectory,
    replaceTheme
  }), [deleteTheme, importTheme, replaceTheme]);

  return {
    ...catalog,
    ...actions,
    capabilities: getAppRuntime().themes.capabilities,
    error,
    loading,
    refresh
  };
}
