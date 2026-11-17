import type { EditorView } from '@codemirror/view';
import { findNoteRelations, linkMentionLine, sameRelationInputs, type NoteRelations, type RelationLine, type RelationNote } from './note-relations';
import type { Library, Note } from './notebook';

const expandedKey = 'qingyu.note-links.expanded';

export function createNoteRelationsFooter(actions: {
  context(): { library: Library; note: Note | undefined };
  open(id: string, line: number): Promise<void>;
  write(id: string, before: string, after: string, target: string): Promise<boolean>;
  report(text: string): void;
}) {
  let view: EditorView | undefined, target: string | undefined;
  let snapshot: Library | undefined, scan: AbortController | undefined;
  let found: NoteRelations = { linked: [], mentions: [] };
  let linkedLimit = 20, mentionLimit = 10, expanded = false, busy = false, timer = 0;
  let changed: { note: RelationNote; before: string; after: string } | undefined;
  try { expanded = localStorage.getItem(expandedKey) === 'true'; } catch { /* Optional visual preference. */ }
  const footer = document.createElement('section'), summary = document.createElement('button'), details = document.createElement('div');
  footer.className = 'note-relations'; footer.ariaLabel = '笔记引用'; footer.contentEditable = 'false';
  summary.className = 'note-relations-summary'; summary.type = 'button';
  details.className = 'note-relations-details';
  footer.append(summary, details);
  summary.onclick = () => {
    expanded = !expanded;
    try { localStorage.setItem(expandedKey, String(expanded)); } catch { /* Optional visual preference. */ }
    render();
  };

  function button(text: string, fn: () => void, className = '') {
    const control = document.createElement('button');
    control.type = 'button'; control.className = className; control.textContent = text; control.onclick = fn;
    return control;
  }
  function render() {
    if (!view || !target) return;
    const { linked, mentions } = found;
    const visible = Boolean(linked.length || mentions.length || changed);
    // WebKit can focus the outer text editor when a user taps a button inside contenteditable.
    if (visible && footer.parentElement !== view.scrollDOM) view.scrollDOM.append(footer);
    else if (!visible) footer.remove();
    if (!visible) return;
    summary.textContent = [
      linked.length ? `${linked.length} 篇笔记链接到这里` : '',
      mentions.length ? `${mentions.length} 篇笔记提及` : '',
    ].filter(Boolean).join(' · ') || '笔记引用';
    summary.ariaExpanded = String(expanded);
    details.hidden = !expanded;
    if (!expanded) { view.requestMeasure(); return; }
    const before = details.contains(document.activeElement) ? document.activeElement?.getAttribute('data-relation-focus') : null;
    details.replaceChildren();
    for (const [title, rows, limit, mention] of [
      ['链接到这里', linked, linkedLimit, false], ['未链接的提及', mentions, mentionLimit, true],
    ] as const) {
      if (!rows.length && !(mention && changed)) continue;
      const section = document.createElement('section'), heading = document.createElement('h3');
      heading.textContent = title; section.append(heading);
      if (mention && changed) {
        const notice = document.createElement('div'); notice.className = 'note-relation-notice';
        notice.append(`已在「${changed.note.title}」添加链接`, button('撤销', () => { void undo(); }, 'note-relation-link'));
        section.append(notice);
      }
      for (const note of rows.slice(0, limit)) {
        const card = document.createElement('article'); card.className = 'note-relation-card';
        const openTitle = button(note.title, () => { void actions.open(note.id, note.lines[0].number); }, 'note-relation-title');
        openTitle.dataset.relationFocus = `${title}:${note.id}`;
        card.append(openTitle);
        for (const line of note.lines) {
          const row = document.createElement('div'); row.className = 'note-relation-line';
          const excerpt = button('', () => { void actions.open(note.id, line.number); }, 'note-relation-excerpt');
          excerpt.dataset.relationFocus = `${title}:${note.id}:${line.number}`;
          let from = 0;
          for (const mark of line.marks) {
            const ink = document.createElement('strong'); ink.textContent = line.text.slice(mark.from, mark.to);
            if (!mention) ink.className = 'note-relation-mark';
            excerpt.append(line.text.slice(from, mark.from), ink); from = mark.to;
          }
          excerpt.append(line.text.slice(from)); row.append(excerpt);
          if (mention) {
            const link = button('添加链接', () => { void linkLine(note, line); }, 'note-relation-link');
            link.ariaLabel = `在${note.title}第 ${line.number} 行添加链接`;
            row.append(link);
          }
          card.append(row);
        }
        section.append(card);
      }
      if (rows.length > limit) section.append(button(`显示全部 ${rows.length} 篇`, () => {
        if (mention) mentionLimit = Infinity; else linkedLimit = Infinity;
        render();
      }, 'note-relation-link'));
      details.append(section);
    }
    if (before !== null) ([...details.querySelectorAll<HTMLButtonElement>('button')].find(el => el.dataset.relationFocus === before) ?? summary).focus({ preventScroll: true });
    view.requestMeasure();
  }

  async function linkLine(other: RelationNote, line: RelationLine) {
    if (busy) return;
    const { library, note } = actions.context();
    const original = library.notes.find(n => n.id === other.id);
    if (!note || note.id !== target || original?.source !== other.source || original.trashed || original.readOnly) {
      actions.report('引用正文已变化，请重试'); return;
    }
    const next = linkMentionLine(library, note, original, line.spots);
    if (!next || next === original.source) { actions.report('无法生成指向此笔记的唯一链接'); return; }
    busy = true;
    try {
      if (!await actions.write(other.id, original.source, next, note.id) || note.id !== target) return;
      changed = { note: other, before: original.source, after: next };
      clearTimeout(timer);
      timer = window.setTimeout(() => { changed = undefined; render(); }, 6000);
      render();
    } finally { busy = false; }
  }

  async function undo() {
    if (!target || !changed || busy) return;
    const edit = changed;
    busy = true;
    try {
      if (await actions.write(edit.note.id, edit.after, edit.before, target) && changed === edit) {
        changed = undefined; clearTimeout(timer); render();
      }
    } finally { busy = false; }
  }

  return {
    refresh(editor: EditorView) {
      view = editor;
      const { library, note } = actions.context();
      const id = note && !note.trashed && !note.readOnly ? note.id : undefined;
      if (target === id && snapshot && (snapshot === library || id && sameRelationInputs(snapshot, library, id))) {
        if (footer.parentElement !== editor.scrollDOM) render();
        snapshot = library;
        return;
      }
      snapshot = library;
      scan?.abort();
      scan = undefined;
      if (target !== id) {
        target = id; changed = undefined; clearTimeout(timer);
        found = { linked: [], mentions: [] }; linkedLimit = 20; mentionLimit = 10;
        footer.remove();
      }
      if (!id || !note) return;
      const running = new AbortController(); scan = running;
      // Start after the editor can paint the new note, then yield between batches.
      window.setTimeout(() => {
        if (running.signal.aborted) return;
        void findNoteRelations(library, note, running.signal).then(result => {
          if (running.signal.aborted) return;
          scan = undefined; found = result; render();
        }).catch(() => { if (!running.signal.aborted) scan = undefined; });
      }, 0);
    },
  };
}
