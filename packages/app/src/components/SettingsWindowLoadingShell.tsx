import { MacWindowControls } from "./MacWindowControls";
import { WindowsWindowControls } from "./WindowsWindowControls";
import { resolveDesktopPlatform } from "../lib/platform";
import { hideSettingsWindow } from "../lib/tauri";
import { getAppRuntime } from "../runtime";
import type { SettingsWindowPresentation } from "../hooks/useSettingsWindowState";
import { SettingsModalFrame } from "./SettingsModalFrame";

const loadingCategoryWidths = ["w-14", "w-20", "w-11", "w-12", "w-16", "w-13", "w-15", "w-18"];
const loadingSectionRows = [1, 2, 1, 1];

function ShimmerBlock({ className }: { className: string }) {
  return <span aria-hidden="true" className={`settings-loading-shimmer ${className}`} />;
}

function SettingsLoadingSidebar({ platform }: { platform: ReturnType<typeof resolveDesktopPlatform> }) {
  const headerClassName = platform === "windows"
    ? "settings-sidebar-header flex h-14 items-center px-7 max-[700px]:hidden"
    : "settings-sidebar-header px-7 pt-14 pb-5 max-[700px]:hidden";
  const sidebarSurfaceClassName = platform === "windows"
    ? "border-r-0 bg-(--bg-chrome)"
    : "border-r border-(--border-default) bg-(--bg-secondary)";

  return (
    <aside
      className={`settings-loading-sidebar settings-sidebar flex min-h-0 flex-col max-[700px]:shrink-0 max-[700px]:border-r-0 max-[700px]:border-b max-[700px]:border-(--border-default) ${sidebarSurfaceClassName}`}
      aria-hidden="true"
    >
      <div className={headerClassName}>
        <ShimmerBlock className="h-4 w-16 rounded" />
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-hidden px-3 max-[700px]:flex-none max-[700px]:flex-row max-[700px]:px-2 max-[700px]:py-2">
        {loadingCategoryWidths.map((widthClassName, index) => (
          <div
            className="flex h-9 w-full items-center gap-3 rounded-md px-3 max-[700px]:w-auto max-[700px]:shrink-0"
            key={`${widthClassName}-${index}`}
          >
            <ShimmerBlock className="size-4 shrink-0 rounded-full" />
            <ShimmerBlock className={`h-3 ${widthClassName} rounded`} />
          </div>
        ))}
      </div>

      <div className="border-t border-(--border-default) px-7 py-4 max-[700px]:hidden">
        <ShimmerBlock className="h-3 w-20 rounded" />
      </div>
    </aside>
  );
}

function SettingsLoadingContent({ platform }: { platform: ReturnType<typeof resolveDesktopPlatform> }) {
  const contentSurfaceClassName = platform === "windows"
    ? "rounded-tl-md border-t border-l border-(--border-default)"
    : "";

  return (
    <section
      className={`settings-loading-content settings-content flex min-h-0 min-w-0 flex-col bg-(--bg-primary) ${contentSurfaceClassName}`}
      aria-hidden="true"
    >
      <header className="settings-content-header flex h-14 shrink-0 items-center border-b border-(--border-default) px-7 max-[700px]:h-12 max-[700px]:px-4">
        <ShimmerBlock className="h-4 w-24 rounded" />
      </header>

      <div className="settings-loading-scroll min-h-0 flex-1 overflow-hidden px-8 py-7">
        {loadingSectionRows.map((rowCount, sectionIndex) => (
          <section className="mb-8 last:mb-0" key={`${rowCount}-${sectionIndex}`}>
            <ShimmerBlock className="mb-3 h-3 w-14 rounded" />
            <div className={rowCount > 1 ? "divide-y divide-(--border-default)" : ""}>
              {Array.from({ length: rowCount }, (_, rowIndex) => (
                <div
                  className="grid min-h-15 grid-cols-[minmax(0,1fr)_auto] items-center gap-5 py-4 max-[520px]:grid-cols-1 max-[520px]:gap-2"
                  key={rowIndex}
                >
                  <div className="min-w-0">
                    <ShimmerBlock className="h-3 w-40 max-w-[62%] rounded" />
                    <ShimmerBlock className="mt-2 h-2 w-72 max-w-[88%] rounded" />
                  </div>
                  <ShimmerBlock
                    className={rowIndex % 2 === 0
                      ? "h-6 w-10 rounded-full"
                      : "h-8 w-22 rounded-md"}
                  />
                </div>
              ))}
            </div>
          </section>
        ))}
      </div>
    </section>
  );
}

export function SettingsWindowLoadingShell({
  onClose,
  presentation = "window"
}: {
  onClose?: () => unknown;
  presentation?: SettingsWindowPresentation;
}) {
  const appRuntime = getAppRuntime();
  const platform = resolveDesktopPlatform();
  const modalPresentation = presentation === "modal";
  const windowsChromeLayout = platform === "windows" && appRuntime.features.nativeWindowChrome;
  const showWindowsWindowChrome = !modalPresentation && windowsChromeLayout;
  const showMacosWindowChrome = !modalPresentation && platform === "macos" && appRuntime.features.nativeWindowChrome;
  const settingsLayoutClassName = windowsChromeLayout
    ? "settings-layout absolute inset-x-0 top-10 bottom-0 grid grid-cols-[180px_minmax(0,1fr)] max-[700px]:grid-cols-1 max-[700px]:grid-rows-[auto_minmax(0,1fr)]"
    : `settings-layout grid ${modalPresentation ? "h-full" : "h-screen"} grid-cols-[180px_minmax(0,1fr)] max-[700px]:grid-cols-1 max-[700px]:grid-rows-[auto_minmax(0,1fr)]`;
  const handleCloseSettings = onClose ?? (() => {
    hideSettingsWindow().catch(() => {});
  });

  const loadingContent = (
    <main
      className={`settings-window-loading relative ${modalPresentation ? "h-full" : "h-screen"} overflow-hidden overscroll-none bg-(--bg-primary) text-(--text-primary)`}
      aria-busy="true"
      aria-label="QingYu settings"
    >
      {showMacosWindowChrome ? (
        <div
          className="settings-drag-region fixed inset-x-0 top-0 z-10 h-9.5 select-none [-webkit-user-select:none]"
          aria-hidden="true"
          data-tauri-drag-region
        />
      ) : null}
      {showMacosWindowChrome ? (
        <MacWindowControls
          className="fixed top-0 left-0 z-20 h-9.5"
          onClose={handleCloseSettings}
        />
      ) : null}
      {windowsChromeLayout ? (
        <header
          className={`settings-window-chrome ${modalPresentation ? "absolute" : "fixed"} inset-x-0 top-0 z-30 grid h-10 grid-cols-[minmax(0,1fr)_auto] select-none items-center bg-(--bg-chrome) [-webkit-user-select:none]`}
          aria-label="QingYu"
          data-tauri-drag-region={showWindowsWindowChrome ? true : undefined}
        >
          <div
            className="relative z-20 flex h-10 items-center px-3 text-[12px] leading-none font-[620] text-(--text-heading)"
            data-tauri-drag-region={showWindowsWindowChrome ? true : undefined}
          >
            QingYu
          </div>
          <div
            className="pointer-events-none absolute top-0 left-1/2 z-10 flex h-10 -translate-x-1/2 items-center justify-center"
            data-tauri-drag-region={showWindowsWindowChrome ? true : undefined}
          >
            <ShimmerBlock className="h-3 w-16 rounded" />
          </div>
          {showWindowsWindowChrome ? <WindowsWindowControls onClose={handleCloseSettings} /> : null}
        </header>
      ) : null}

      <div className={settingsLayoutClassName}>
        <SettingsLoadingSidebar platform={platform} />
        <SettingsLoadingContent platform={platform} />
      </div>
    </main>
  );

  if (!modalPresentation) return loadingContent;

  return (
    <SettingsModalFrame
      label="Settings"
      onClose={handleCloseSettings}
      platform={platform}
    >
      {loadingContent}
    </SettingsModalFrame>
  );
}
