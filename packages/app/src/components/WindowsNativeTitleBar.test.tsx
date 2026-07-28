import { render } from "@testing-library/react";
import { WindowsNativeTitleBar } from "./WindowsNativeTitleBar";

describe("WindowsNativeTitleBar theme contract", () => {
  it("uses the stable titlebar and chrome-border surfaces", () => {
    const { container } = render(
      <WindowsNativeTitleBar
        dirty={false}
        documentKind="file"
        documentName="Draft.md"
        historyDisabled={false}
        label={(key) => key}
        markdownFilesButtonVisible
        markdownFilesOpen={false}
        markdownFilesResizing={false}
        markdownFilesWidth={288}
        nativeWindowChrome
        saveDisabled={false}
        sourceMode={false}
        sourceModeDisabled={false}
        themeActionLabel="Switch theme"
        titlebarSideSlotWidth={196}
        renderDocumentActions={(className) => <div className={className} />}
        renderTitleContent={(className) => <div className={className}>Draft.md</div>}
        onOpenMarkdown={() => {}}
        onSaveMarkdown={() => {}}
        onToggleMarkdownFiles={() => {}}
        onToggleTheme={() => {}}
      />
    );

    expect(container.querySelector(".windows-app-chrome")).toHaveClass(
      "theme-titlebar-legacy-chrome",
      "theme-titlebar-surface"
    );
    expect(container.querySelector(".native-titlebar")).toHaveClass(
      "theme-titlebar-legacy-primary",
      "theme-titlebar-surface"
    );
    expect(container.querySelector(".native-titlebar")).toHaveClass("theme-chrome-border");
  });
});
