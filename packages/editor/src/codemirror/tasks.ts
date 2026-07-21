import type { EditorState, Range } from "@codemirror/state";
import { Decoration, EditorView, WidgetType } from "@codemirror/view";

class TaskWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly markerFrom: number,
  ) {
    super();
  }

  eq(other: TaskWidget) {
    return (
      other.checked === this.checked && other.markerFrom === this.markerFrom
    );
  }

  toDOM(view: EditorView) {
    const checkbox = document.createElement("input");
    checkbox.className = "cm-markra-task-checkbox";
    checkbox.type = "checkbox";
    checkbox.checked = this.checked;
    checkbox.setAttribute(
      "aria-label",
      this.checked ? "Mark task incomplete" : "Mark task complete",
    );

    checkbox.addEventListener("change", () => {
      const markerTo = this.markerFrom + 3;
      const currentMarker = view.state.doc.sliceString(
        this.markerFrom,
        markerTo,
      );

      // A stale widget may briefly receive an event while an external edit is
      // being rendered. Never overwrite text unless it is still a task marker.
      if (!/^\[[ xX]\]$/.test(currentMarker)) return;

      view.dispatch({
        changes: {
          from: this.markerFrom,
          to: markerTo,
          insert: checkbox.checked ? "[x]" : "[ ]",
        },
      });
    });

    return checkbox;
  }
}

export function createTaskDecoration(
  state: EditorState,
  from: number,
  to: number,
): Range<Decoration> | null {
  const marker = state.doc.sliceString(from, to);
  if (!/^\[[ xX]\]$/.test(marker)) return null;

  const trailingSpace = state.doc.sliceString(to, to + 1) === " ";
  return Decoration.replace({
    widget: new TaskWidget(marker[1]?.toLowerCase() === "x", from),
  }).range(from, trailingSpace ? to + 1 : to);
}
