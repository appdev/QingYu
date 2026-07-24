import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import type {
  CompactNavigation,
  CompactSettingsCategory
} from "../../hooks/useCompactNavigation";
import { defaultMcpConfig, type McpSettingsSnapshot } from "../../lib/mcp";
import { defaultEditorPreferences, type EditorPreferences } from "../../lib/settings/app-settings";
import { protectedThemeDescriptors } from "../../lib/themes/theme-catalog";
import type { AppMcpRuntime } from "../../runtime";
import { CompactSettingsDetail } from "./CompactSettingsDetail";
import type { CompactAppController } from "./types";

function navigation(category: CompactSettingsCategory, onBack: () => unknown = () => undefined): CompactNavigation {
  return {
    canGoBack: true,
    page: { kind: "settings-detail", category },
    pop: vi.fn(async () => {
      onBack();
      return true;
    }),
    popIfCurrent: vi.fn(async () => true),
    popToEditor: vi.fn(async () => true),
    push: vi.fn(() => true),
    replace: vi.fn(() => true),
    stack: [{ kind: "editor" }, { kind: "settings" }, { kind: "settings-detail", category }]
  };
}

function mcpSnapshot(): McpSettingsSnapshot {
  return {
    clientCommand: null,
    config: defaultMcpConfig(),
    endpoint: null,
    health: { state: "stopped", endpoint: null, errorCode: null },
    revision: "mobile-revision",
    workspace: null
  };
}

function mcpRuntime(): AppMcpRuntime {
  return {
    clearAuditEntries: vi.fn(async () => undefined),
    getHealth: vi.fn(async () => mcpSnapshot().health),
    getSettings: vi.fn(async () => mcpSnapshot()),
    listAuditEntries: vi.fn(async () => []),
    localServiceAvailable: false,
    policyAvailable: true,
    setPrimaryWorkspace: vi.fn(async () => mcpSnapshot()),
    updateSettings: vi.fn(async ({ config }) => ({ ...mcpSnapshot(), config }))
  };
}

function controller(overrides: Partial<CompactAppController> = {}) {
  return {
    appearance: {
      activeTheme: protectedThemeDescriptors[0],
      appearanceMode: "system",
      catalog: {
        capabilities: { canDelete: true, canImport: false, canOpenDirectory: false },
        darkThemes: [protectedThemeDescriptors[1]],
        deleteTheme: vi.fn(async () => undefined),
        error: null,
        importTheme: vi.fn(async () => null),
        invalidFiles: [],
        lightThemes: [protectedThemeDescriptors[0]],
        loading: false,
        openDirectory: vi.fn(async () => undefined),
        refresh: vi.fn(async () => undefined),
        replaceTheme: vi.fn(async () => protectedThemeDescriptors[0]),
        themes: [...protectedThemeDescriptors]
      },
      darkTheme: "dark",
      lightTheme: "light",
      selectAppearanceMode: vi.fn(),
      selectTheme: vi.fn(),
      themeError: null
    },
    capabilities: {
      imageImport: false,
      openLocalAttachments: false,
      applicationSync: true,
      mcpPolicy: true,
      systemFonts: false,
      trueMobile: false
    },
    files: {
      files: [],
      sourcePath: "/Users/ying/Notes"
    },
    language: "en",
    mcp: mcpRuntime(),
    preferences: {
      loading: false,
      preferences: defaultEditorPreferences,
      updatePreferences: vi.fn()
    },
    workspace: {
      openNotebookManager: vi.fn(),
      primaryRoot: "/Users/ying/Notes",
      syncConfigDocument: null
    },
    selectLanguage: vi.fn(),
    ...overrides
  } as unknown as CompactAppController;
}

describe("CompactSettingsDetail", () => {
  it("supports third-party selection, refresh, and deletion without mobile import or export", async () => {
    const nord = {
      appearance: "dark" as const,
      fileName: "nord",
      fingerprint: "a".repeat(64),
      id: "nord",
      name: "Nord",
      preview: { accent: "#88c0d0", background: "#2e3440", panel: "#3b4252", text: "#eceff4" },
      source: "third-party" as const,
      storageKind: "resourceDirectory" as const
    };
    const setup = controller();
    setup.appearance.catalog.darkThemes = [protectedThemeDescriptors[1], nord];
    setup.appearance.catalog.themes = [...setup.appearance.catalog.lightThemes, ...setup.appearance.catalog.darkThemes];
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<CompactSettingsDetail category="appearance" controller={setup} navigation={navigation("appearance")} />);

    await waitFor(() => expect(setup.appearance.catalog.refresh).toHaveBeenCalledTimes(1));

    fireEvent.click(within(screen.getByRole("radiogroup", { name: "Dark palette" }))
      .getByRole("radio", { name: "Nord" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh themes" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete theme: Nord" }));

    expect(setup.appearance.selectTheme).toHaveBeenCalledWith(nord);
    expect(setup.appearance.catalog.refresh).toHaveBeenCalledTimes(2);
    expect(setup.appearance.catalog.deleteTheme).toHaveBeenCalledWith(nord);
    expect(screen.queryByRole("button", { name: "Import theme" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open theme folder" })).not.toBeInTheDocument();
  });

  it("shows the true-mobile managed root, local totals, mode, and notebook manager entry", () => {
    const openNotebookManager = vi.fn();
    const setup = controller({
      capabilities: {
        imageImport: true,
        openLocalAttachments: false,
        applicationSync: true,
        mcpPolicy: true,
        systemFonts: false,
        trueMobile: true
      },
      files: {
        files: [
          { kind: "folder", name: "docs", path: "/mobile/workspace/docs", relativePath: "docs" },
          { name: "one.md", path: "/mobile/workspace/one.md", relativePath: "one.md", sizeBytes: 1024 },
          { name: "two.md", path: "/mobile/workspace/two.md", relativePath: "two.md", sizeBytes: 2048 }
        ],
        sourcePath: "/mobile/workspace"
      } as CompactAppController["files"],
      workspace: {
        openNotebookManager,
        primaryRoot: "/mobile/workspace",
        syncConfigDocument: null
      }
    });

    render(<CompactSettingsDetail category="storage" controller={setup} navigation={navigation("storage")} />);

    const storage = screen.getByRole("region", { name: "Storage" });
    expect(storage).toHaveClass("absolute", "inset-0", "overflow-x-hidden");
    expect(storage.querySelector("header")).toHaveClass("pt-[var(--compact-safe-area-top)]");
    expect(storage.querySelector("[data-compact-settings-scroll]"))
      .toHaveAttribute("data-compact-scroll", "vertical");
    expect(storage.querySelector("[data-compact-settings-scroll]"))
      .toHaveClass("overflow-y-auto", "overflow-x-hidden", "pb-[calc(1.25rem+var(--compact-bottom-inset))]");
    expect(screen.getByText("App-managed workspace")).toBeInTheDocument();
    expect(screen.getByText("/mobile/workspace")).toBeInTheDocument();
    expect(screen.getByText("Local files").nextElementSibling).toHaveTextContent("2");
    expect(screen.getByText("Total size").nextElementSibling).toHaveTextContent("3 KB");
    expect(screen.getByText("Workspace mode").nextElementSibling).toHaveTextContent("Local workspace");
    expect(storage.querySelector("input")).not.toBeInTheDocument();
    expect(storage.querySelector("select")).not.toBeInTheDocument();
    expect(screen.queryByText(/Browse|Reveal|Reset|Clear|Delete Cloud|Import|Export/i))
      .not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Manage notebooks" }));
    expect(openNotebookManager).toHaveBeenCalledOnce();
  });

  it("identifies a narrow-desktop root and routes its notebook switch action", () => {
    const openNotebookManager = vi.fn();
    render(
      <CompactSettingsDetail
        category="storage"
        controller={controller({
          workspace: {
            openNotebookManager,
            primaryRoot: "/Users/ying/Notes",
            syncConfigDocument: null
          }
        })}
        navigation={navigation("storage")}
      />
    );

    expect(screen.getByText("Current project path")).toBeInTheDocument();
    expect(screen.getByText("/Users/ying/Notes")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Switch Notebook Directory" }));
    expect(openNotebookManager).toHaveBeenCalledOnce();
  });

  it("shows sync mode only for an enabled, ready applied project and still excludes folders from totals", () => {
    const setup = controller({
      files: {
        files: [
          { kind: "folder", name: "docs", path: "/notes/docs", relativePath: "docs", sizeBytes: 4096 },
          { name: "one.md", path: "/notes/one.md", relativePath: "one.md", sizeBytes: 512 }
        ],
        sourcePath: "/notes"
      } as CompactAppController["files"],
      workspace: {
        openNotebookManager: vi.fn(),
        primaryRoot: "/notes",
        syncConfigDocument: {
          config: {
            enabled: true
          },
          readiness: "ready"
        }
      } as unknown as CompactAppController["workspace"]
    });

    render(<CompactSettingsDetail category="storage" controller={setup} navigation={navigation("storage")} />);

    expect(screen.getByText("Local files").nextElementSibling).toHaveTextContent("1");
    expect(screen.getByText("Total size").nextElementSibling).toHaveTextContent("512 B");
    expect(screen.getByText("Workspace mode").nextElementSibling).toHaveTextContent("Sync workspace");
  });

  it.each([
    ["general", "Language", "zh-CN", "zh-CN"],
    ["appearance", "Dark", "dark", "dark"],
    ["editor", "Body font size", "18", "18"]
  ] as const)("keeps %s updates when returning to Settings", (category, controlName, action, retainedValue) => {
    function StatefulDetail() {
      const [page, setPage] = useState<"detail" | "settings">("detail");
      const [language, setLanguage] = useState<CompactAppController["language"]>("en");
      const [appearanceMode, setAppearanceMode] = useState<"system" | "light" | "dark">("system");
      const [preferences, setPreferences] = useState<EditorPreferences>(defaultEditorPreferences);
      const retained = category === "general"
        ? language
        : category === "appearance"
          ? appearanceMode
          : String(preferences.bodyFontSize);

      if (page === "settings") return <p>Settings retained: {retained}</p>;

      return (
        <CompactSettingsDetail
          category={category}
          controller={controller({
            appearance: {
              ...controller().appearance,
              appearanceMode,
              selectAppearanceMode: setAppearanceMode
            },
            language,
            preferences: { loading: false, preferences, updatePreferences: setPreferences },
            selectLanguage: setLanguage
          })}
          navigation={navigation(category, () => setPage("settings"))}
        />
      );
    }

    render(<StatefulDetail />);
    if (category === "general" || category === "editor") {
      fireEvent.change(screen.getByRole("combobox", { name: controlName }), { target: { value: action } });
    } else {
      fireEvent.click(within(screen.getByRole("radiogroup", { name: "Appearance mode" }))
        .getByRole("radio", { name: controlName }));
    }
    fireEvent.click(screen.getByRole("button", { name: category === "general" ? "返回" : "Back" }));

    expect(screen.getByText(`Settings retained: ${retainedValue}`)).toBeInTheDocument();
  });

  it("omits system font controls on mobile while keeping approved editor preferences", () => {
    render(
      <CompactSettingsDetail category="editor" controller={controller()} navigation={navigation("editor")} />
    );

    [
      "Body font size",
      "Line height",
      "Paragraph spacing",
      "Wrap code block lines"
    ].forEach((name) => expect(screen.getByLabelText(name)).toBeInTheDocument());
    expect(screen.queryByLabelText("Editor font")).not.toBeInTheDocument();
    expect(screen.queryByText(/View mode|Content width|Source|Split|Tabs|AI|Templates|Shortcuts/i))
      .not.toBeInTheDocument();
    const editorRegion = screen.getByRole("region", { name: "Editor" });
    Array.from(editorRegion.querySelectorAll("button, input, select"))
      .forEach((control) => expect(control).toHaveClass("min-h-11", "min-w-11"));
  });

  it("keeps the custom font control for Compact runtimes that support system fonts", () => {
    render(
      <CompactSettingsDetail
        category="editor"
        controller={controller({
          capabilities: {
            imageImport: false,
            openLocalAttachments: true,
          applicationSync: true,
          mcpPolicy: true,
            systemFonts: true,
            trueMobile: false
          }
        })}
        navigation={navigation("editor")}
      />
    );

    expect(screen.getByLabelText("Editor font")).toBeInTheDocument();
  });

  it("renders the shared application MCP policy as a mobile-safe responsive detail", async () => {
    const setup = controller({
      capabilities: {
        imageImport: true,
        openLocalAttachments: false,
        applicationSync: true,
        mcpPolicy: true,
        systemFonts: false,
        trueMobile: true
      },
      mcp: mcpRuntime()
    });

    render(<CompactSettingsDetail category="mcp" controller={setup} navigation={navigation("mcp")} />);

    const detail = screen.getByRole("region", { name: "MCP" });
    expect(detail.querySelector("[data-compact-settings-scroll]"))
      .toHaveClass("overflow-y-auto", "pb-[calc(1.25rem+var(--compact-bottom-inset))]");
    expect(await screen.findByRole("button", { name: "Enable MCP" })).toBeInTheDocument();
    expect(screen.queryByText("Health")).not.toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });
});
