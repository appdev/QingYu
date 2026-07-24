import {
  ChevronDown,
  ChevronRight,
  FilePlus2,
  FileText,
  Folder,
  FolderPlus,
  MoreHorizontal
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { t, type I18nKey } from "@markra/shared";
import type { NativeMarkdownFolderFile } from "../../lib/tauri";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import {
  buildMarkdownFileTree,
  buildVisibleFileTreeRows,
  filterMarkdownFileTree,
  folderNodeAsFile
} from "../file-tree/file-tree-model";
import type { CompactFileBrowserController } from "./types";
import { CompactNameDialog, compactNameOperationErrorMessage } from "./CompactNameDialog";

type CompactFileBrowserScreenProps = {
  controller: CompactFileBrowserController;
  navigation: CompactNavigation;
};

const compactTargetClass = "min-h-11 min-w-11";
const longPressDelayMs = 500;

type CompactFileNameAction =
  | { kind: "create-file"; parentPath: string | null }
  | { kind: "create-folder"; parentPath: string | null }
  | { file: NativeMarkdownFolderFile; kind: "rename" };

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export function CompactFileBrowserScreen({
  controller,
  navigation
}: CompactFileBrowserScreenProps) {
  const language = controller.language ?? "en";
  const translate = (key: I18nKey) => t(language, key);
  const [actionPath, setActionPath] = useState<string | null>(null);
  const [nameAction, setNameAction] = useState<CompactFileNameAction | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(() => new Set());
  const [searchQuery, setSearchQuery] = useState("");
  const longPressTimerRef = useRef<number | null>(null);
  const longPressTriggeredRef = useRef(false);
  const tree = useMemo(
    () => buildMarkdownFileTree(controller.files.files, controller.files.sourcePath),
    [controller.files.files, controller.files.sourcePath]
  );
  const filteredTree = useMemo(
    () => filterMarkdownFileTree(tree, searchQuery),
    [searchQuery, tree]
  );
  const rows = useMemo(
    () => buildVisibleFileTreeRows(
      filteredTree,
      expandedFolders,
      searchQuery,
      false,
      false,
      null
    ),
    [expandedFolders, filteredTree, searchQuery]
  );

  const clearLongPressTimer = () => {
    if (longPressTimerRef.current === null) return;
    window.clearTimeout(longPressTimerRef.current);
    longPressTimerRef.current = null;
  };

  const cancelPendingLongPress = () => {
    if (longPressTimerRef.current === null) return;
    clearLongPressTimer();
    longPressTriggeredRef.current = false;
  };

  const startLongPress = (file: NativeMarkdownFolderFile) => {
    clearLongPressTimer();
    longPressTriggeredRef.current = false;
    longPressTimerRef.current = window.setTimeout(() => {
      longPressTimerRef.current = null;
      longPressTriggeredRef.current = true;
      setActionPath(file.path);
    }, longPressDelayMs);
  };

  const finishLongPress = () => {
    clearLongPressTimer();
  };

  useEffect(() => () => {
    clearLongPressTimer();
  }, []);

  const reportError = (operationError: unknown) => {
    setError(errorMessage(operationError, translate("compact.files.operationFailed")));
  };

  const attemptNavigationFlush = async () => {
    try {
      await controller.saveState.flush("navigation");
    } catch {
      // Continue after the write attempt; the editor keeps the persistent save failure.
    }
  };

  const openFile = async (file: NativeMarkdownFolderFile) => {
    setError(null);
    await attemptNavigationFlush();
    const opened = await controller.files.openFile(file);
    if (opened === false) return;
    await navigation.popToEditor();
  };

  const submitNameAction = async (name: string) => {
    if (!nameAction) return;
    setError(null);

    if (nameAction.kind === "create-file") {
      const file = await controller.files.createFile(name, nameAction.parentPath);
      if (!file) throw new Error("File creation failed");

      await attemptNavigationFlush();
      await controller.files.openFile(file);
      await navigation.popToEditor();
      setNameAction(null);
      return;
    }

    if (nameAction.kind === "create-folder") {
      const folder = await controller.files.createFolder(name, nameAction.parentPath);
      if (!folder) throw new Error("Folder creation failed");
      setNameAction(null);
      return;
    }

    if (name === nameAction.file.name) {
      setActionPath(null);
      setNameAction(null);
      return;
    }

    await controller.files.renameFile(nameAction.file, name);
    setActionPath(null);
    setNameAction(null);
  };

  const deleteFile = async (file: NativeMarkdownFolderFile) => {
    setActionPath(null);
    setError(null);
    await controller.files.deleteFile(file);
  };

  const toggleFolder = (relativePath: string) => {
    setExpandedFolders((currentFolders) => {
      const nextFolders = new Set(currentFolders);
      if (nextFolders.has(relativePath)) {
        nextFolders.delete(relativePath);
      } else {
        nextFolders.add(relativePath);
      }
      return nextFolders;
    });
  };

  const activateRow = (file: NativeMarkdownFolderFile, folder: boolean, relativePath: string) => {
    if (longPressTriggeredRef.current) {
      longPressTriggeredRef.current = false;
      return;
    }

    if (folder) {
      toggleFolder(relativePath);
      return;
    }
    openFile(file).catch(reportError);
  };

  return (
    <section
      aria-label={translate("compact.files.title")}
      className="absolute inset-0 flex h-full min-h-0 w-full flex-col bg-(--bg-primary)"
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-(--border-subtle) px-2 pt-[var(--compact-safe-area-top)]">
        <button
          aria-label={translate("compact.navigation.back")}
          className={`${compactTargetClass} rounded-lg px-3 text-sm`}
          type="button"
          onClick={() => navigation.pop().catch(reportError)}
        >
          {translate("compact.navigation.back")}
        </button>
        <h1 className="min-w-0 flex-1 truncate text-base font-semibold">
          {translate("compact.files.title")}
        </h1>
        <button
          aria-label={translate("compact.files.newFile")}
          className={`${compactTargetClass} inline-flex items-center justify-center rounded-lg`}
          type="button"
          onClick={() => setNameAction({ kind: "create-file", parentPath: null })}
        >
          <FilePlus2 aria-hidden="true" size={20} />
        </button>
        <button
          aria-label={translate("compact.files.newFolder")}
          className={`${compactTargetClass} inline-flex items-center justify-center rounded-lg`}
          type="button"
          onClick={() => setNameAction({ kind: "create-folder", parentPath: null })}
        >
          <FolderPlus aria-hidden="true" size={20} />
        </button>
      </header>

      <div className="shrink-0 p-3">
        <input
          aria-label={translate("compact.files.search")}
          className="min-h-11 w-full rounded-lg border border-(--border-subtle) bg-(--bg-secondary) px-3 text-base"
          placeholder={translate("compact.files.search")}
          type="search"
          value={searchQuery}
          onChange={(event) => setSearchQuery(event.currentTarget.value)}
        />
      </div>

      {error ? <p className="mx-3 mb-2 text-sm text-(--status-error)" role="alert">{error}</p> : null}

      <div
        className="min-h-0 flex-1 overflow-y-auto px-2 pb-[calc(1rem+var(--compact-bottom-inset))]"
        data-compact-scroll="vertical"
      >
        {rows.map((row) => {
          if (row.type === "create") return null;

          const file = row.node.type === "folder" ? folderNodeAsFile(row.node) : row.node.file;
          const folder = row.node.type === "folder";
          const actionsOpen = actionPath === file.path;

          return (
            <div key={row.key} style={{ paddingLeft: `${row.depth * 16}px` }}>
              <div className="flex min-h-11 items-stretch gap-1">
                <button
                  aria-expanded={folder ? row.expanded : undefined}
                  className={`${compactTargetClass} flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 text-left`}
                  type="button"
                  onClick={() => activateRow(file, folder, row.node.relativePath)}
                  onPointerCancel={cancelPendingLongPress}
                  onPointerDown={() => startLongPress(file)}
                  onPointerMove={cancelPendingLongPress}
                  onPointerUp={finishLongPress}
                >
                  {folder ? (
                    row.expanded
                      ? <ChevronDown aria-hidden="true" className="shrink-0" size={18} />
                      : <ChevronRight aria-hidden="true" className="shrink-0" size={18} />
                  ) : <span aria-hidden="true" className="w-[18px]" />}
                  {folder
                    ? <Folder aria-hidden="true" className="shrink-0" size={20} />
                    : <FileText aria-hidden="true" className="shrink-0" size={20} />}
                  <span className="truncate">{row.node.name}</span>
                </button>
                <button
                  aria-expanded={actionsOpen}
                  aria-label={`${translate("compact.files.actions")}: ${file.name}`}
                  className={`${compactTargetClass} inline-flex shrink-0 items-center justify-center rounded-lg`}
                  type="button"
                  onClick={() => setActionPath(actionsOpen ? null : file.path)}
                >
                  <MoreHorizontal aria-hidden="true" size={20} />
                </button>
              </div>

              {actionsOpen ? (
                <div
                  aria-label={`${translate("compact.files.actions")}: ${file.name}`}
                  className="grid grid-cols-2 gap-1 pb-2"
                  role="group"
                >
                  {folder ? (
                    <>
                      <button
                        className={`${compactTargetClass} rounded-lg px-3 text-left text-sm`}
                        type="button"
                        onClick={() => setNameAction({ kind: "create-file", parentPath: file.path })}
                      >
                        {translate("compact.files.newFileHere")}
                      </button>
                      <button
                        className={`${compactTargetClass} rounded-lg px-3 text-left text-sm`}
                        type="button"
                        onClick={() => setNameAction({ kind: "create-folder", parentPath: file.path })}
                      >
                        {translate("compact.files.newFolderHere")}
                      </button>
                    </>
                  ) : null}
                  <button
                    aria-label={`${translate("compact.files.rename")} ${file.name}`}
                    className={`${compactTargetClass} rounded-lg px-3 text-left text-sm`}
                    type="button"
                    onClick={() => {
                      setNameAction({ file, kind: "rename" });
                    }}
                  >
                    {translate("compact.files.rename")}
                  </button>
                  <button
                    aria-label={`${translate("compact.files.move")} ${file.name}`}
                    className={`${compactTargetClass} rounded-lg px-3 text-left text-sm`}
                    type="button"
                    onClick={() => {
                      setActionPath(null);
                      navigation.push({ kind: "move-target", path: file.path });
                    }}
                  >
                    {translate("compact.files.move")}
                  </button>
                  <button
                    aria-label={`${translate("compact.files.delete")} ${file.name}`}
                    className={`${compactTargetClass} rounded-lg px-3 text-left text-sm text-(--status-error)`}
                    type="button"
                    onClick={() => deleteFile(file).catch(reportError)}
                  >
                    {translate("compact.files.delete")}
                  </button>
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
      {nameAction ? (
        <CompactNameDialog
          cancelLabel={translate("compact.files.cancel")}
          errorMessage={(operationError) => compactNameOperationErrorMessage(operationError, {
            duplicate: translate("compact.files.nameExists"),
            fallback: translate("compact.files.operationFailed"),
            invalid: translate("compact.files.nameInvalid")
          })}
          initialValue={nameAction.kind === "rename" ? nameAction.file.name : ""}
          submitLabel={nameAction.kind === "rename"
            ? translate("compact.files.rename")
            : translate("compact.files.create")}
          title={nameAction.kind === "create-folder"
            ? translate("compact.files.newFolderName")
            : nameAction.kind === "rename"
              ? `${translate("compact.files.rename")} ${nameAction.file.name}`
              : translate("compact.files.newFileName")}
          onCancel={() => setNameAction(null)}
          onSubmit={submitNameAction}
        />
      ) : null}
    </section>
  );
}
