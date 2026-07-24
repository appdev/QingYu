import { fireEvent, render, screen } from "@testing-library/react";
import { SelectionToolbar } from "./SelectionToolbar";

const anchor = {
  bottom: 180,
  left: 240,
  right: 420,
  top: 148
};

describe("SelectionToolbar", () => {
  it("renders ordinary formatting actions", () => {
    render(
      <SelectionToolbar
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    expect(screen.getByRole("toolbar", { name: "Format" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Link" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
  });

  it("places itself below the selection near the viewport top", () => {
    render(
      <SelectionToolbar
        anchor={{
          bottom: 30,
          left: 240,
          right: 420,
          top: 10
        }}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    const toolbar = screen.getByRole("toolbar", { name: "Format" });

    expect(toolbar).toHaveStyle({
      left: "330px",
      top: "42px"
    });
    expect(toolbar).toHaveClass("translate-y-0");
    expect(toolbar).not.toHaveClass("-translate-y-full");
  });

  it("renders basic selection tools and routes them to editor commands", () => {
    const onRunFormattingAction = vi.fn();
    const onInsertLink = vi.fn();
    const onCopySelection = vi.fn();

    render(
      <SelectionToolbar
        anchor={anchor}
        language="en"
        open
        onCopySelection={onCopySelection}
        onInsertLink={onInsertLink}
        onRunFormattingAction={onRunFormattingAction}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    fireEvent.click(screen.getByRole("button", { name: "Italic" }));
    fireEvent.click(screen.getByRole("button", { name: "Strikethrough" }));
    fireEvent.click(screen.getByRole("button", { name: "Inline Code" }));
    fireEvent.click(screen.getByRole("button", { name: "Highlight" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear Formatting" }));
    fireEvent.click(screen.getByRole("button", { name: "Quote" }));
    fireEvent.click(screen.getByRole("button", { name: "Bullet List" }));
    fireEvent.click(screen.getByRole("button", { name: "Ordered List" }));
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));

    expect(onRunFormattingAction.mock.calls.map(([action]) => action)).toEqual([
      "bold",
      "italic",
      "strikethrough",
      "inlineCode",
      "highlight",
      "clearFormatting",
      "quote",
      "bulletList",
      "orderedList"
    ]);
    expect(onInsertLink).toHaveBeenCalledTimes(1);
    expect(onCopySelection).toHaveBeenCalledTimes(1);
  });

  it("marks active formatting tools as pressed", () => {
    render(
      <SelectionToolbar
        activeFormattingActions={["bold", "highlight", "link"]}
        activeHeadingLevel={1}
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Highlight" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Heading Level H1" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("combobox", { name: "Heading Level" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Link" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Italic" })).toHaveAttribute("aria-pressed", "false");
  });

  it("routes heading level choices from the toolbar", () => {
    const onSetHeadingLevel = vi.fn();

    render(
      <SelectionToolbar
        activeHeadingLevel={2}
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
        onSetHeadingLevel={onSetHeadingLevel}
      />
    );

    const headingButton = screen.getByRole("button", { name: "Heading Level H2" });
    expect(headingButton).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(headingButton);

    expect(headingButton).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("menu", { name: "Heading Level" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "H1" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "H6" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("menuitemradio", { name: "H3" }));

    expect(onSetHeadingLevel).toHaveBeenCalledWith(3);
  });

  it("routes paragraph choices from the heading level menu", () => {
    const onRunFormattingAction = vi.fn();

    render(
      <SelectionToolbar
        activeHeadingLevel={2}
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={onRunFormattingAction}
        onSetHeadingLevel={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Heading Level H2" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Paragraph" }));

    expect(onRunFormattingAction).toHaveBeenCalledWith("paragraph");
    expect(screen.queryByRole("menu", { name: "Heading Level" })).not.toBeInTheDocument();
  });

  it("renders the heading level menu outside the scrollable toolbar shell", () => {
    render(
      <SelectionToolbar
        activeHeadingLevel={2}
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
        onSetHeadingLevel={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Heading Level H2" }));

    const headingMenu = screen.getByRole("menu", { name: "Heading Level" });

    expect(headingMenu.closest(".overflow-x-auto")).toBeNull();
    expect(headingMenu).toHaveClass("fixed");
  });

  it("keeps the heading level menu inside the viewport near the bottom edge", () => {
    const originalInnerHeight = window.innerHeight;
    const originalInnerWidth = window.innerWidth;

    Object.defineProperty(window, "innerHeight", { configurable: true, value: 640 });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 800 });

    try {
      render(
        <SelectionToolbar
          activeHeadingLevel={2}
          anchor={anchor}
          language="en"
          open
          onCopySelection={vi.fn()}
          onInsertLink={vi.fn()}
          onRunFormattingAction={vi.fn()}
          onSetHeadingLevel={vi.fn()}
        />
      );

      const headingButton = screen.getByRole("button", { name: "Heading Level H2" });
      vi.spyOn(headingButton, "getBoundingClientRect").mockReturnValue({
        bottom: 628,
        height: 32,
        left: 120,
        right: 152,
        top: 596,
        width: 32,
        x: 120,
        y: 596,
        toJSON: () => ({})
      });

      fireEvent.click(headingButton);

      const headingMenu = screen.getByRole("menu", { name: "Heading Level" });
      expect(Number.parseInt(headingMenu.style.top, 10)).toBeLessThan(596);
      expect(headingMenu.style.maxHeight).toBe("584px");
      expect(headingMenu.style.overflowY).toBe("auto");
    } finally {
      Object.defineProperty(window, "innerHeight", { configurable: true, value: originalInnerHeight });
      Object.defineProperty(window, "innerWidth", { configurable: true, value: originalInnerWidth });
    }
  });

  it("keeps the heading level menu available outside headings", () => {
    const { rerender } = render(
      <SelectionToolbar
        activeHeadingLevel={null}
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
        onSetHeadingLevel={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Heading Level" })).toBeEnabled();

    rerender(
      <SelectionToolbar
        activeHeadingLevel={1}
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
        onSetHeadingLevel={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Heading Level H1" })).toBeEnabled();
  });

  it("shows the copy button success state in place", () => {
    render(
      <SelectionToolbar
        anchor={anchor}
        copySucceeded
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
  });

  it("dismisses when a pointer starts outside the toolbar", () => {
    const onDismiss = vi.fn();

    render(
      <SelectionToolbar
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onDismiss={onDismiss}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    fireEvent.pointerDown(screen.getByRole("button", { name: "Bold" }));
    expect(onDismiss).not.toHaveBeenCalled();

    fireEvent.pointerDown(document.body);

    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("does not render when there is no selected text anchor", () => {
    render(
      <SelectionToolbar
        anchor={null}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    expect(screen.queryByRole("toolbar", { name: "Format" })).not.toBeInTheDocument();
  });
});
