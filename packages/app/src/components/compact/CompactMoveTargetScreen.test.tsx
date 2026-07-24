import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { vi } from "vitest";
import { useCompactNavigation, type CompactNavigation } from "../../hooks/useCompactNavigation";
import { CompactMoveTargetScreen } from "./CompactMoveTargetScreen";
import type { CompactAppController } from "./types";

describe("CompactMoveTargetScreen", () => {
  it("lands on Files when Back already occurred before a deferred move resolves", async () => {
    const files = [
      { path: "/vault/docs/note.md", name: "note.md", relativePath: "docs/note.md" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    let finishMove: (() => unknown) | undefined;
    const moveFile = vi.fn(() => new Promise<boolean>((resolve) => {
      finishMove = () => resolve(true);
    }));
    const controller = { files: { files, moveFile, sourcePath: "/vault" } } as unknown as CompactAppController;

    function NavigationHarness() {
      const navigation = useCompactNavigation();
      useEffect(() => {
        navigation.push({ kind: "files" });
        navigation.push({ kind: "move-target", path: files[0].path });
      }, []);

      return (
        <>
          <button type="button" onClick={() => navigation.pop().catch(() => {})}>Native Back</button>
          {navigation.page.kind === "move-target" ? (
            <CompactMoveTargetScreen
              controller={controller}
              navigation={navigation}
              path={navigation.page.path}
            />
          ) : navigation.page.kind === "files" ? <p>Files page</p> : null}
        </>
      );
    }

    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<NavigationHarness />);
    fireEvent.click(await screen.findByRole("button", { name: "archive" }));
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(moveFile).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Native Back" }));
    await screen.findByText("Files page");
    await act(async () => finishMove?.());

    expect(screen.getByText("Files page")).toBeInTheDocument();
    expect(moveFile).toHaveBeenCalledTimes(1);
  });

  it("offers the root and valid folders, moves exactly once, then returns to files", async () => {
    const files = [
      { kind: "folder" as const, path: "/vault/docs", name: "docs", relativePath: "docs" },
      { kind: "folder" as const, path: "/vault/docs/drafts", name: "drafts", relativePath: "docs/drafts" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    let finishMove: (() => unknown) | undefined;
    const moveFile = vi.fn(() => new Promise<boolean>((resolve) => {
      finishMove = () => resolve(true);
    }));
    const navigation = { popIfCurrent: vi.fn().mockResolvedValue(true) } as unknown as CompactNavigation;
    const controller = {
      files: { files, moveFile, sourcePath: "/vault" }
    } as unknown as CompactAppController;

    render(
      <CompactMoveTargetScreen
        controller={controller}
        navigation={navigation}
        path="/vault/docs/drafts"
      />
    );

    expect(screen.getByRole("button", { name: "Project root" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "archive" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "docs" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "drafts" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "archive" }));
    fireEvent.click(screen.getByRole("button", { name: "archive" }));
    expect(moveFile).toHaveBeenCalledTimes(1);
    expect(moveFile).toHaveBeenCalledWith(files[1], "/vault/archive");
    expect(navigation.popIfCurrent).not.toHaveBeenCalled();

    finishMove?.();
    await waitFor(() => expect(navigation.popIfCurrent).toHaveBeenCalledWith({
      kind: "move-target",
      path: "/vault/docs/drafts"
    }));
  });

  it("disables Back and destinations while a move is pending", async () => {
    const files = [
      { path: "/vault/docs/note.md", name: "note.md", relativePath: "docs/note.md" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    let finishMove: (() => unknown) | undefined;
    const moveFile = vi.fn(() => new Promise<boolean>((resolve) => {
      finishMove = () => resolve(true);
    }));
    const navigation = {
      pop: vi.fn().mockResolvedValue(true),
      popIfCurrent: vi.fn().mockResolvedValue(true)
    } as unknown as CompactNavigation;
    const controller = { files: { files, moveFile, sourcePath: "/vault" } } as unknown as CompactAppController;

    render(<CompactMoveTargetScreen controller={controller} navigation={navigation} path={files[0].path} />);
    fireEvent.click(screen.getByRole("button", { name: "archive" }));

    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "archive" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(navigation.pop).not.toHaveBeenCalled();

    await act(async () => finishMove?.());
  });

  it("stays open after a resolved move failure and allows a successful retry", async () => {
    const files = [
      { path: "/vault/docs/note.md", name: "note.md", relativePath: "docs/note.md" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    const moveFile = vi.fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const navigation = {
      pop: vi.fn().mockResolvedValue(true),
      popIfCurrent: vi.fn().mockResolvedValue(true)
    } as unknown as CompactNavigation;
    const controller = { files: { files, moveFile, sourcePath: "/vault" } } as unknown as CompactAppController;

    render(<CompactMoveTargetScreen controller={controller} navigation={navigation} path={files[0].path} />);
    fireEvent.click(screen.getByRole("button", { name: "archive" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("The item could not be moved. Try again.");
    expect(navigation.popIfCurrent).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Back" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "archive" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "archive" }));
    await waitFor(() => expect(navigation.popIfCurrent).toHaveBeenCalledTimes(1));
    expect(moveFile).toHaveBeenCalledTimes(2);
  });

  it("omits the current Project root destination for a root-level item", () => {
    const file = { path: "/vault/readme.md", name: "readme.md", relativePath: "readme.md" };
    const navigation = { pop: vi.fn().mockResolvedValue(true) } as unknown as CompactNavigation;
    const controller = {
      files: { files: [file], moveFile: vi.fn(), sourcePath: "/vault" }
    } as unknown as CompactAppController;

    render(<CompactMoveTargetScreen controller={controller} navigation={navigation} path={file.path} />);
    expect(screen.queryByRole("button", { name: "Project root" })).not.toBeInTheDocument();
  });

  it("uses a single 44px Back action", () => {
    const navigation = { pop: vi.fn().mockResolvedValue(true) } as unknown as CompactNavigation;
    const controller = {
      files: {
        files: [{ path: "/vault/readme.md", name: "readme.md", relativePath: "readme.md" }],
        moveFile: vi.fn(),
        sourcePath: "/vault"
      }
    } as unknown as CompactAppController;

    render(
      <CompactMoveTargetScreen
        controller={controller}
        navigation={navigation}
        path="/vault/readme.md"
      />
    );

    const back = screen.getByRole("button", { name: "Back" });
    const page = screen.getByRole("region", { name: "Move to" });
    expect(screen.getAllByRole("button", { name: "Back" })).toHaveLength(1);
    expect(back).toHaveClass("min-h-11", "min-w-11");
    expect(page.querySelector("header")).toHaveClass("pt-[var(--compact-safe-area-top)]");
    expect(page.querySelector('[data-compact-scroll="vertical"]'))
      .toHaveClass("pb-[calc(0.5rem+var(--compact-bottom-inset))]");
    fireEvent.click(back);
    expect(navigation.pop).toHaveBeenCalledTimes(1);
  });

  it("localizes the move destination and fallback failure in Simplified Chinese", async () => {
    const files = [
      { path: "/vault/docs/note.md", name: "note.md", relativePath: "docs/note.md" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    const navigation = {
      pop: vi.fn().mockResolvedValue(true),
      popIfCurrent: vi.fn().mockResolvedValue(true)
    } as unknown as CompactNavigation;
    const controller = {
      files: {
        files,
        moveFile: vi.fn().mockResolvedValue(false),
        sourcePath: "/vault"
      },
      language: "zh-CN"
    } as unknown as CompactAppController;

    render(<CompactMoveTargetScreen controller={controller} navigation={navigation} path={files[0].path} />);

    expect(screen.getByRole("region", { name: "移动到" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "返回" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "移动到" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "项目根目录" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "archive" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("无法移动此项目，请重试。");
  });
});
