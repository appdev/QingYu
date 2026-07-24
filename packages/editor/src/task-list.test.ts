import {
  defaultValueCtx,
  Editor,
  editorViewCtx,
  remarkStringifyOptionsCtx,
  rootCtx,
  serializerCtx
} from "@milkdown/kit/core";
import { commonmark } from "@milkdown/kit/preset/commonmark";
import { gfm } from "@milkdown/kit/preset/gfm";
import { TextSelection } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import { markraTaskListSchema, toggleTaskListInView } from "./task-list";

async function createEditor(markdown: string) {
  const root = document.createElement("div");
  document.body.append(root);
  const editor = await Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, root);
      ctx.set(defaultValueCtx, markdown);
      ctx.update(remarkStringifyOptionsCtx, (options) => ({
        ...options,
        bullet: "-" as const
      }));
    })
    .use(commonmark)
    .use(gfm)
    .use(markraTaskListSchema)
    .create();
  const view = editor.action((ctx) => ctx.get(editorViewCtx));
  let textPosition: number | null = null;

  view.state.doc.descendants((node, position) => {
    if (!node.isText || textPosition !== null) return true;
    textPosition = position;
    return false;
  });

  if (textPosition === null) throw new Error("Expected an editor document containing text.");

  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, textPosition)));

  return {
    destroy: () => editor.destroy(),
    serialize: () => editor.action((ctx) => ctx.get(serializerCtx)(view.state.doc)),
    view
  };
}

function runTaskListCommand(view: EditorView) {
  return toggleTaskListInView(view, {
    bulletList: view.state.schema.nodes.bullet_list,
    listItem: view.state.schema.nodes.list_item,
    orderedList: view.state.schema.nodes.ordered_list
  });
}

function findTextRange(view: EditorView, text: string) {
  let range: { from: number; to: number } | null = null;

  view.state.doc.descendants((node, position) => {
    if (!node.isText || range) return true;

    const offset = node.text?.indexOf(text) ?? -1;
    if (offset < 0) return true;

    range = {
      from: position + offset,
      to: position + offset + text.length
    };
    return false;
  });

  if (!range) throw new Error(`Expected editor text containing "${text}".`);
  return range;
}

function placeCursorIn(view: EditorView, text: string) {
  const { from } = findTextRange(view, text);
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, from)));
}

function selectThrough(view: EditorView, fromText: string, toText: string) {
  const { from } = findTextRange(view, fromText);
  const { to } = findTextRange(view, toText);
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, from, to)));
}

describe("task-list conversion", () => {
  it("converts a paragraph to an unchecked task item and serializes it as task Markdown", async () => {
    const editor = await createEditor("First item");

    try {
      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.textContent).toBe("First item");
      expect(editor.view.state.doc.firstChild?.type.name).toBe("bullet_list");
      expect(editor.view.state.doc.firstChild?.firstChild?.attrs.checked).toBe(false);
      expect(editor.serialize()).toBe("- [ ] First item\n");
    } finally {
      await editor.destroy();
    }
  });

  it("converts a normal list to unchecked task items without changing its text", async () => {
    const editor = await createEditor("- First item\n- Second item");

    try {
      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.textContent).toBe("First itemSecond item");
      expect(editor.view.state.doc.firstChild?.firstChild?.attrs.checked).toBe(false);
      expect(editor.view.state.doc.firstChild?.lastChild?.attrs.checked).toBe(false);
      expect(editor.serialize()).toBe("- [ ] First item\n- [ ] Second item\n");
    } finally {
      await editor.destroy();
    }
  });

  it("toggles a selected task list back to a normal bullet list", async () => {
    const editor = await createEditor("- [ ] First item\n- [x] Second item");

    try {
      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.textContent).toBe("First itemSecond item");
      expect(editor.view.state.doc.firstChild?.type.name).toBe("bullet_list");
      expect(editor.view.state.doc.firstChild?.firstChild?.attrs.checked).toBeNull();
      expect(editor.view.state.doc.firstChild?.lastChild?.attrs.checked).toBeNull();
      expect(editor.serialize()).toBe("- First item\n- Second item\n");
    } finally {
      await editor.destroy();
    }
  });

  it("uses the common outer list when a selection crosses from a nested item to an outer sibling", async () => {
    const editor = await createEditor("- Parent\n  - Nested\n- Sibling");

    try {
      selectThrough(editor.view, "Nested", "Sibling");

      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.textContent).toBe("ParentNestedSibling");

      const outerList = editor.view.state.doc.firstChild;
      const firstOuterItem = outerList?.firstChild;
      const nestedList = firstOuterItem?.lastChild;
      expect(outerList?.type.name).toBe("bullet_list");
      expect(outerList?.childCount).toBe(2);
      expect(firstOuterItem?.attrs.checked).toBe(false);
      expect(outerList?.lastChild?.attrs.checked).toBe(false);
      expect(nestedList?.type.name).toBe("bullet_list");
      expect(nestedList?.firstChild?.attrs.checked).toBeNull();
      expect(editor.serialize()).toBe("- [ ] Parent\n  - Nested\n- [ ] Sibling\n");
    } finally {
      await editor.destroy();
    }
  });

  it("toggles only the selected task item in a mixed list back to normal", async () => {
    const editor = await createEditor("- [x] Task item\n- Normal item");

    try {
      placeCursorIn(editor.view, "Task item");

      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.firstChild?.firstChild?.attrs.checked).toBeNull();
      expect(editor.view.state.doc.firstChild?.lastChild?.attrs.checked).toBeNull();
      expect(editor.serialize()).toBe("- Task item\n- Normal item\n");
    } finally {
      await editor.destroy();
    }
  });

  it("converts only the selected normal item in a mixed list to a task", async () => {
    const editor = await createEditor("- [x] Task item\n- Normal item");

    try {
      placeCursorIn(editor.view, "Normal item");

      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.firstChild?.firstChild?.attrs.checked).toBe(true);
      expect(editor.view.state.doc.firstChild?.lastChild?.attrs.checked).toBe(false);
      expect(editor.serialize()).toBe("- [x] Task item\n- [ ] Normal item\n");
    } finally {
      await editor.destroy();
    }
  });

  it("preserves checked task items when a mixed selection also converts normal items", async () => {
    const editor = await createEditor("- [x] Completed task\n- Normal item");

    try {
      selectThrough(editor.view, "Completed task", "Normal item");

      expect(runTaskListCommand(editor.view)).toBe(true);
      expect(editor.view.state.doc.firstChild?.firstChild?.attrs.checked).toBe(true);
      expect(editor.view.state.doc.firstChild?.lastChild?.attrs.checked).toBe(false);
      expect(editor.serialize()).toBe("- [x] Completed task\n- [ ] Normal item\n");
    } finally {
      await editor.destroy();
    }
  });
});
