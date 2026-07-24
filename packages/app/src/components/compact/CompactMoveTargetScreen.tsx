import { Folder, FolderRoot } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { t, type I18nKey } from "@markra/shared";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import { sameNativePath } from "../../lib/path-move";
import {
  buildMarkdownFileTree,
  collectMarkdownMoveTargets
} from "../file-tree/file-tree-model";
import type { CompactFileBrowserController } from "./types";

type CompactMoveTargetScreenProps = {
  controller: CompactFileBrowserController;
  navigation: CompactNavigation;
  path: string;
};

const compactTargetClass = "min-h-11 min-w-11";

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export function CompactMoveTargetScreen({
  controller,
  navigation,
  path
}: CompactMoveTargetScreenProps) {
  const language = controller.language ?? "en";
  const translate = (key: I18nKey) => t(language, key);
  const [error, setError] = useState<string | null>(null);
  const [moving, setMoving] = useState(false);
  const movingRef = useRef(false);
  const movingFile = useMemo(
    () => controller.files.files.find((file) => sameNativePath(file.path, path)) ?? null,
    [controller.files.files, path]
  );
  const targets = useMemo(() => {
    const tree = buildMarkdownFileTree(controller.files.files, controller.files.sourcePath);
    return collectMarkdownMoveTargets(tree, movingFile, controller.files.sourcePath);
  }, [controller.files.files, controller.files.sourcePath, movingFile]);

  const moveTo = async (targetPath: string | null) => {
    if (!movingFile || movingRef.current) return;

    movingRef.current = true;
    setMoving(true);
    setError(null);
    try {
      const moved = await controller.files.moveFile(movingFile, targetPath);
      if (!moved) {
        movingRef.current = false;
        setMoving(false);
        setError(translate("compact.files.moveFailed"));
        return;
      }

      await navigation.popIfCurrent({ kind: "move-target", path });
    } catch (moveError) {
      movingRef.current = false;
      setMoving(false);
      setError(errorMessage(moveError, translate("compact.files.moveFailed")));
    }
  };

  return (
    <section
      aria-label={translate("compact.files.moveDestination")}
      className="absolute inset-0 flex h-full min-h-0 w-full flex-col bg-(--bg-primary)"
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-(--border-subtle) px-2 pt-[var(--compact-safe-area-top)]">
        <button
          aria-label={translate("compact.navigation.back")}
          className={`${compactTargetClass} rounded-lg px-3 text-sm`}
          disabled={moving}
          type="button"
          onClick={() => navigation.pop().catch((navigationError) => {
            setError(errorMessage(navigationError, translate("compact.files.moveFailed")));
          })}
        >
          {translate("compact.navigation.back")}
        </button>
        <h1 className="min-w-0 flex-1 truncate text-base font-semibold">
          {translate("compact.files.moveDestination")}
        </h1>
      </header>

      {error ? <p className="mx-3 mt-3 text-sm text-(--status-error)" role="alert">{error}</p> : null}
      {!movingFile ? (
        <p className="p-4 text-sm text-(--text-secondary)" role="status">
          {translate("compact.files.itemUnavailable")}
        </p>
      ) : (
        <div
          className="min-h-0 flex-1 overflow-y-auto p-2 pb-[calc(0.5rem+var(--compact-bottom-inset))]"
          data-compact-scroll="vertical"
        >
          {targets.map((target) => {
            const name = target.kind === "root"
              ? translate("compact.files.projectRoot")
              : target.node.name;
            return (
              <button
                key={target.path ?? "__project_root__"}
                className={`${compactTargetClass} flex w-full items-center gap-3 rounded-lg px-3 text-left disabled:opacity-50`}
                disabled={moving}
                style={{ paddingLeft: `${12 + target.depth * 16}px` }}
                type="button"
                onClick={() => moveTo(target.path)}
              >
                {target.kind === "root"
                  ? <FolderRoot aria-hidden="true" className="shrink-0" size={20} />
                  : <Folder aria-hidden="true" className="shrink-0" size={20} />}
                <span className="truncate">{name}</span>
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
