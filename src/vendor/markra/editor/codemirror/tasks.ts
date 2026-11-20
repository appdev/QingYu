import { isolateHistory } from "@codemirror/commands";
import { Facet, type EditorState, type Range } from "@codemirror/state";
import { Decoration, EditorView, WidgetType } from "@codemirror/view";

// QingYu's split preview delegates task changes to its editable parent.
export const previewTask = Facet.define<(line: number) => void>();
export const checklistToggleEvent = "input.task.toggle";

export function focusTaskCheckbox(view: EditorView, position: number) {
  const { node } = view.domAtPos(position);
  const element = node instanceof Element ? node : node.parentElement;
  element?.closest(".cm-line")?.querySelector<HTMLInputElement>(".cm-markra-task-checkbox")?.focus({ preventScroll: true });
}

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

  coordsAt(dom: HTMLElement, offset: number) {
    if (offset === 0) return null;
    // CodeMirror's default widget bounds exclude the gap before the task text.
    const rect = dom.getBoundingClientRect();
    const style = getComputedStyle(dom);
    const gap = parseFloat(style.marginInlineEnd);
    const end = style.direction === "rtl" ? rect.left - gap : rect.right + gap;
    return new DOMRect(end, rect.top, 0, rect.height);
  }

  ignoreEvent(event: Event) {
    if (event.type === "contextmenu") return false;
    if (event instanceof KeyboardEvent && event.type === "keydown" &&
      (event.metaKey || event.ctrlKey) && /^[zy]$/i.test(event.key)) return false;
    return true;
  }

  toDOM(view: EditorView) {
    const checkbox = document.createElement("input");
    checkbox.className = "cm-markra-task-checkbox";
    checkbox.dataset.taskMarker = String(this.markerFrom);
    checkbox.type = "checkbox";
    checkbox.checked = this.checked;
    checkbox.disabled = view.state.readOnly && !view.state.facet(previewTask).length;
    checkbox.setAttribute(
      "aria-label",
      this.checked ? "标为未完成" : "标为已完成",
    );

    checkbox.onmousedown = event => {
      if (event.button !== 0) return;
      event.preventDefault();
      if (!view.hasFocus && !checkbox.ownerDocument.documentElement.hasAttribute("data-mobile")) {
        checkbox.focus({ preventScroll: true });
      }
    };
    // WebKit can activate a checkbox after opening its secondary-click menu.
    checkbox.onauxclick = event => event.preventDefault();
    checkbox.addEventListener("change", () => {
      const markerTo = this.markerFrom + 3;
      const currentMarker = view.state.doc.sliceString(
        this.markerFrom,
        markerTo,
      );

      // A stale widget may briefly receive an event while an external edit is
      // being rendered. Never overwrite text unless it is still a task marker.
      if (!/^\[[ xX]\]$/.test(currentMarker)) return;
      if (view.state.readOnly) {
        checkbox.checked = this.checked;
        view.state.facet(previewTask)[0]?.(view.state.doc.lineAt(this.markerFrom).number);
        return;
      }

      const focused = view.root.activeElement === checkbox;
      view.dispatch({
        changes: {
          from: this.markerFrom,
          to: markerTo,
          insert: checkbox.checked ? "[x]" : "[ ]",
        },
        annotations: isolateHistory.of("before"),
        userEvent: checklistToggleEvent,
      });
      if (focused && !checkbox.isConnected && !checkbox.ownerDocument.documentElement.hasAttribute("data-mobile")) {
        focusTaskCheckbox(view, this.markerFrom);
      }
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
