import type { EditorView } from "@codemirror/view";
import {
  createContext,
  useContext,
  useMemo,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { getMarkraEditorReactStore } from "./bridge.ts";

interface MarkraEditorContextValue {
  revision: number;
  view: EditorView | null;
}

export interface MarkraEditorProviderProps {
  children?: ReactNode;
  view: EditorView | null;
}

const MarkraEditorContext = createContext<MarkraEditorContextValue | null>(null);
const emptySubscribe = (_listener: () => unknown) => () => false;
const emptySnapshot = () => 0;

export function MarkraEditorProvider({
  children,
  view,
}: MarkraEditorProviderProps) {
  const store = view ? getMarkraEditorReactStore(view) : null;
  const revision = useSyncExternalStore(
    store?.subscribe ?? emptySubscribe,
    store?.getSnapshot ?? emptySnapshot,
    emptySnapshot,
  );
  const value = useMemo(() => ({ revision, view }), [revision, view]);

  return (
    <MarkraEditorContext.Provider value={value}>
      {children}
    </MarkraEditorContext.Provider>
  );
}

export function useMarkraEditorContext() {
  const context = useContext(MarkraEditorContext);
  if (!context) {
    throw new Error(
      "Markra editor React hooks must be used inside MarkraEditorProvider",
    );
  }
  return context;
}

export function useMarkraEditorView() {
  return useMarkraEditorContext().view;
}
