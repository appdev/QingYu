import {
  useId,
  useLayoutEffect,
  useRef,
  type CSSProperties,
  type RefObject
} from "react";
import { t, type AppLanguage } from "@markra/shared";
import featherAsset from "../../../../assets/branding/app-icon/feather.png";
import {
  contrastRatio,
  ensureContrast,
  fitContrast,
  parseComputedRgb,
  rgbColorValue,
  type RgbColor
} from "../lib/workspace-home-contrast";

type WorkspaceHomeShortcut =
  | "createDocument"
  | "openDocument"
  | "quickOpen"
  | "openSettings"
  | "showFiles";

export type WorkspaceHomeProps = {
  language: AppLanguage;
  presentation: "desktop" | "compact";
  actions: {
    createDocument: () => unknown;
    openDocument?: () => unknown;
    quickOpen?: () => unknown;
    showFiles?: () => unknown;
    openSettings?: () => unknown;
    configureSync?: () => unknown;
    switchWorkspace?: () => unknown;
  };
  shortcuts?: Partial<Record<WorkspaceHomeShortcut, string>>;
};

type WorkspaceHomeAction = {
  callback: (() => unknown) | undefined;
  key: keyof WorkspaceHomeProps["actions"];
  labelKey: string;
  shortcutKey?: WorkspaceHomeShortcut;
};

const darkSurface: RgbColor = [35, 40, 45];
const lightSurface: RgbColor = [255, 255, 255];
const darkText: RgbColor = [231, 233, 234];
const lightText: RgbColor = [38, 38, 38];
const darkMuted: RgbColor = [171, 178, 191];
const lightMuted: RgbColor = [107, 107, 107];

function resolveCssRgb(scope: HTMLElement, value: string): RgbColor | null {
  const normalized = value.trim();
  if (!normalized || /gradient\(|url\(|^transparent$/iu.test(normalized)) return null;

  const probe = document.createElement("span");
  probe.hidden = true;
  probe.style.color = normalized;
  if (!probe.style.color) return null;

  scope.append(probe);
  const computedColor = getComputedStyle(probe).color;
  probe.remove();
  const resolved = parseComputedRgb(computedColor);
  if (resolved) return resolved;

  const canvas = document.createElement("canvas");
  canvas.width = 1;
  canvas.height = 1;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return null;

  context.clearRect(0, 0, 1, 1);
  context.fillStyle = computedColor;
  context.fillRect(0, 0, 1, 1);
  const [red, green, blue, alpha] = context.getImageData(0, 0, 1, 1).data;
  return alpha === 255 ? [red, green, blue] as RgbColor : null;
}

function applyWorkspaceHomeContrast(home: HTMLElement) {
  const root = document.documentElement;
  const style = getComputedStyle(root);
  const dark = root.dataset.themeAppearance === "dark" || style.colorScheme.includes("dark");
  const background = resolveCssRgb(home, style.getPropertyValue("--bg-primary"))
    ?? (dark ? darkSurface : lightSurface);
  const preferred = resolveCssRgb(home, style.getPropertyValue("--text-heading"))
    ?? (dark ? darkText : lightText);
  const text = resolveCssRgb(home, style.getPropertyValue("--text-primary")) ?? preferred;
  const muted = resolveCssRgb(home, style.getPropertyValue("--text-secondary"))
    ?? (dark ? darkMuted : lightMuted);
  const accent = resolveCssRgb(home, style.getPropertyValue("--accent")) ?? preferred;
  const brandBase = fitContrast(background, preferred, 1.55);
  const brandSlice = fitContrast(background, preferred, 2.05);

  home.style.setProperty("--workspace-home-brand-base", rgbColorValue(brandBase));
  home.style.setProperty("--workspace-home-brand-slice", rgbColorValue(brandSlice));
  home.style.setProperty("--workspace-home-text", rgbColorValue(ensureContrast(background, text, 4.5)));
  home.style.setProperty("--workspace-home-muted", rgbColorValue(ensureContrast(background, muted, 4.5)));
  home.style.setProperty("--workspace-home-focus", rgbColorValue(ensureContrast(background, accent, 3)));
  home.dataset.brandBaseContrast = contrastRatio(background, brandBase).toFixed(2);
  home.dataset.brandSliceContrast = contrastRatio(background, brandSlice).toFixed(2);
}

function useWorkspaceHomeContrast(homeRef: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const home = homeRef.current;
    if (!home) return;

    const applyContrast = () => applyWorkspaceHomeContrast(home);
    applyContrast();

    const rootObserver = new MutationObserver(applyContrast);
    rootObserver.observe(document.documentElement, {
      attributeFilter: ["class", "data-theme", "data-theme-appearance", "style"],
      attributes: true
    });
    const themeSourceObserver = new MutationObserver(applyContrast);
    themeSourceObserver.observe(document.head, {
      childList: true,
      characterData: true,
      subtree: true
    });

    return () => {
      rootObserver.disconnect();
      themeSourceObserver.disconnect();
    };
  }, [homeRef]);
}

function BrandMark({ compact }: { compact: boolean }) {
  const maskSize = compact ? "14rem 14rem" : "23rem 23rem";
  const maskStyle: CSSProperties = {
    WebkitMaskImage: `url(${featherAsset})`,
    WebkitMaskPosition: "center",
    WebkitMaskRepeat: "no-repeat",
    WebkitMaskSize: maskSize,
    maskImage: `url(${featherAsset})`,
    maskPosition: "center",
    maskRepeat: "no-repeat",
    maskSize
  };

  return (
    <div
      className={`relative ${compact ? "h-56 w-36" : "h-[23rem] w-60"}`}
      data-workspace-home-brand="sliced-feather"
      aria-hidden="true"
    >
      <span
        className="pointer-events-none absolute inset-0 bg-(--workspace-home-brand-base)"
        style={maskStyle}
      />
      <span
        className="pointer-events-none absolute inset-0 translate-x-[5px] bg-(--workspace-home-brand-slice) [clip-path:inset(0_0_66%_0)]"
        style={maskStyle}
      />
      <span
        className="pointer-events-none absolute inset-0 -translate-x-1 bg-(--workspace-home-brand-slice) [clip-path:inset(33%_0_33%_0)]"
        style={maskStyle}
      />
      <span
        className="pointer-events-none absolute inset-0 translate-x-0.5 bg-(--workspace-home-brand-slice) [clip-path:inset(66%_0_0_0)]"
        style={maskStyle}
      />
    </div>
  );
}

export function WorkspaceHome({
  actions,
  language,
  presentation,
  shortcuts
}: WorkspaceHomeProps) {
  const titleId = useId();
  const homeRef = useRef<HTMLElement>(null);
  const compact = presentation === "compact";
  useWorkspaceHomeContrast(homeRef);

  const actionGroups: WorkspaceHomeAction[][] = [
    [
      {
        callback: actions.createDocument,
        key: "createDocument",
        labelKey: "workspaceHome.createDocument",
        shortcutKey: "createDocument"
      },
      {
        callback: actions.openDocument,
        key: "openDocument",
        labelKey: "workspaceHome.openDocument",
        shortcutKey: "openDocument"
      },
      {
        callback: actions.quickOpen,
        key: "quickOpen",
        labelKey: "workspaceHome.quickOpen",
        shortcutKey: "quickOpen"
      },
      {
        callback: actions.showFiles,
        key: "showFiles",
        labelKey: "workspaceHome.showFiles",
        shortcutKey: "showFiles"
      }
    ],
    [
      {
        callback: actions.openSettings,
        key: "openSettings",
        labelKey: "workspaceHome.openSettings",
        shortcutKey: "openSettings"
      },
      {
        callback: actions.configureSync,
        key: "configureSync",
        labelKey: "workspaceHome.configureSync"
      },
      {
        callback: actions.switchWorkspace,
        key: "switchWorkspace",
        labelKey: "workspaceHome.switchWorkspace"
      }
    ]
  ];
  const availableGroups = actionGroups
    .map((group) => group.filter((action) => action.callback))
    .filter((group) => group.length > 0);
  const homeStyle = {
    "--workspace-home-brand-base": "color-mix(in oklab, var(--bg-primary) 78%, var(--text-heading))",
    "--workspace-home-brand-slice": "color-mix(in oklab, var(--bg-primary) 68%, var(--text-heading))",
    "--workspace-home-focus": "var(--accent)",
    "--workspace-home-muted": "var(--text-secondary)",
    "--workspace-home-text": "var(--text-primary)"
  } as CSSProperties;

  return (
    <section
      className={`flex h-full min-h-0 w-full overflow-y-auto bg-(--bg-primary) text-(--workspace-home-text) ${
        compact
          ? "items-start px-7 pt-8 pb-12"
          : "items-start px-8 py-10 sm:px-12"
      }`}
      data-presentation={presentation}
      data-workspace-surface="home"
      ref={homeRef}
      style={homeStyle}
      aria-labelledby={titleId}
    >
      <div
        className={`mx-auto grid min-w-0 w-full ${
          compact
            ? "max-w-sm gap-7"
            : "my-auto max-w-[32.5rem] -translate-x-6 gap-8"
        }`}
      >
        <h1 className="sr-only" id={titleId}>{t(language, "workspaceHome.title")}</h1>

        <header
          className={`flex w-full items-center justify-start ${compact ? "pl-12" : "pl-16"}`}
          aria-hidden="true"
        >
          <BrandMark compact={compact} />
        </header>

        <div className="grid min-w-0 gap-2">
          {availableGroups.map((group, groupIndex) => (
            <div className="contents" key={group[0].key}>
              {groupIndex > 0 ? (
                <div
                  className="mx-2 h-px bg-(--border-default)"
                  role="separator"
                />
              ) : null}
              <div className="grid min-w-0 gap-0.5">
                {group.map(({ callback, key, labelKey, shortcutKey }) => {
                  if (!callback) return null;
                  const shortcut = !compact && shortcutKey ? shortcuts?.[shortcutKey] : undefined;

                  return (
                    <button
                      className={`group relative grid min-w-0 w-full cursor-pointer grid-cols-[minmax(0,1fr)_auto] items-center gap-4 rounded-md border-0 bg-transparent px-4 text-left text-[13px] leading-5 font-[560] text-(--workspace-home-text) outline-none transition-[transform,background-color] duration-150 ease-out before:absolute before:inset-y-2 before:left-0 before:w-0.5 before:scale-y-50 before:rounded-full before:bg-(--workspace-home-focus) before:opacity-0 before:transition-[opacity,transform] before:duration-150 hover:bg-(--bg-hover) hover:before:scale-y-100 hover:before:opacity-100 focus-visible:ring-2 focus-visible:ring-(--workspace-home-focus) focus-visible:before:scale-y-100 focus-visible:before:opacity-100 active:translate-y-px active:bg-(--bg-active) ${
                        compact ? "min-h-11 min-w-11" : "min-h-10"
                      }`}
                      key={key}
                      type="button"
                      onClick={() => {
                        callback();
                      }}
                    >
                      <span className="min-w-0 truncate">{t(language, labelKey)}</span>
                      {shortcut ? (
                        <kbd
                          className="min-w-10 shrink-0 whitespace-nowrap rounded-sm border border-(--border-default) bg-(--bg-secondary) px-1.5 py-0.5 text-center font-sans text-[10px] leading-4 font-[520] text-(--workspace-home-muted)"
                          aria-hidden="true"
                        >
                          {shortcut}
                        </kbd>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
