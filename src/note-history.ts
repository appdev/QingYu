import { Compartment, EditorState, StateEffect, Text } from '@codemirror/state';
import { Decoration, EditorView } from '@codemirror/view';
import { assetBase } from './attachments';
import { diffVersionLines } from './note-version-diff';
import { noteTitle, type Note, type Notebook, type NoteVersion } from './notebook';
import type { RichEditor } from './rich-editor';
import { markraLanguage } from './vendor/markra/editor/codemirror';

const CURRENT = 'current';
const date = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'numeric', day: 'numeric' });
const time = new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
function day(timestamp: number) {
  const yesterday = new Date(); yesterday.setDate(yesterday.getDate() - 1);
  const value = date.format(timestamp);
  return value === date.format(new Date()) ? '今天' : value === date.format(yesterday) ? '昨天' : value;
}
const heading = ({ id, updatedAt }: NoteVersion) => id === CURRENT ? '当前版本' : `${day(updatedAt)} ${time.format(updatedAt)}`;
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

function showSourceComparison(doc: Text, changed: Set<number>) {
  let hasBlockChanges = false;
  markraLanguage.language.parser.parse(doc.toString()).iterate({
    enter(node) {
      if (hasBlockChanges) return false;
      if (!['Table', 'FencedCode', 'CodeBlock', 'HTMLBlock'].includes(node.name)) return;
      const end = doc.lineAt(Math.max(node.from, node.to - 1)).number;
      for (let line = doc.lineAt(node.from).number; line <= end; line++) {
        if (changed.has(line)) { hasBlockChanges = true; break; }
      }
      return false;
    },
  });
  return hasBlockChanges;
}

export function showNoteHistory(book: Notebook, note: Note, restoreFocus: () => void, restored: () => Promise<void>) {
  if (document.querySelector('.note-history[open]')) return;
  const root = book.root, noteId = note.id;
  const dialog = document.createElement('dialog');
  dialog.className = 'note-history';
  dialog.innerHTML = `<aside class="history-sidebar"><header><h2 id="note-history-title">版本历史</h2><p class="history-note-title"></p></header>
    <div class="history-versions" role="group" aria-label="历史版本"></div></aside>
    <button class="history-close" type="button" aria-label="关闭版本历史" autofocus><span>×</span><span>完成</span></button>
    <div class="history-state"><div role="status"><h3></h3><p></p></div><button type="button" class="button secondary-button" hidden>重试</button></div>
    <section class="history-detail" aria-label="版本预览" hidden><header>
      <button class="history-back" type="button" aria-label="返回版本列表">‹</button>
      <div class="history-summary"><h3></h3><p></p></div>
      <select aria-label="对比版本"></select><button class="history-restore button primary-button" type="button">恢复此版本</button>
    </header><p class="history-error" role="alert" hidden></p><div id="note-history-editor"></div></section>`;
  dialog.ariaLabel = '版本历史';
  const $ = <T extends HTMLElement>(selector: string) => dialog.querySelector<T>(selector)!;
  const list = $('.history-versions'), state = $('.history-state'), summary = $('.history-summary p');
  const close = $<HTMLButtonElement>('.history-close'), back = $<HTMLButtonElement>('.history-back');
  const retry = $<HTMLButtonElement>('.history-state button'), restore = $<HTMLButtonElement>('.history-restore');
  const detail = $('.history-detail'), preview = $('#note-history-editor'), errorLine = $('.history-error');
  const compare = $<HTMLSelectElement>('select');
  const current = { id: CURRENT, updatedAt: note.updatedAt };
  let versions: NoteVersion[] = [], selected = CURRENT, currentSource = note.source, request = 0;
  let editorReady: Promise<RichEditor> | undefined, marksInstalled = false, restoring = false;
  let sourceView: EditorView | undefined;
  const sources = new Map<string, string>(), marks = new Compartment();
  const narrow = matchMedia('(max-width: 760px)');
  const compact = () => dialog.hasAttribute('data-compact');
  const updateLayout = () => {
    dialog.toggleAttribute('data-compact', document.documentElement.hasAttribute('data-mobile') || narrow.matches);
    if (!compact() && versions.length && detail.hidden) select(selected);
  };
  $('.history-note-title').textContent = noteTitle(note);
  close.onclick = () => dialog.close();
  back.onclick = () => {
    dialog.removeAttribute('data-preview');
    list.querySelector<HTMLButtonElement>('[aria-current=true]')?.focus();
  };
  retry.onclick = () => { void load(); };
  compare.onchange = () => { void renderPreview(); };
  restore.onclick = async () => {
    if (restoring || selected === CURRENT) return;
    restoring = true; restore.disabled = compare.disabled = close.disabled = back.disabled = true;
    restore.textContent = '正在恢复…';
    try {
      await book.restoreNoteVersion(noteId, selected, currentSource, root);
      dialog.close();
      await restored();
    } catch (reason) {
      errorLine.textContent = message(reason); errorLine.hidden = false;
    } finally {
      restoring = false; restore.textContent = '恢复此版本';
      restore.disabled = compare.disabled = close.disabled = back.disabled = false;
    }
  };
  dialog.addEventListener('cancel', event => {
    if (restoring) event.preventDefault();
    else if (compact() && dialog.hasAttribute('data-preview')) { event.preventDefault(); back.click(); }
  });
  dialog.addEventListener('keydown', event => event.stopPropagation());
  dialog.addEventListener('close', () => {
    narrow.removeEventListener('change', updateLayout);
    request++; sourceView?.destroy(); EditorView.findFromDOM(preview)?.destroy(); dialog.remove(); restoreFocus();
  }, { once: true });
  list.onkeydown = event => {
    if (!(event.target instanceof HTMLButtonElement) || !['ArrowUp', 'ArrowDown'].includes(event.key)) return;
    const rows = [...list.querySelectorAll('button')], next = rows[rows.indexOf(event.target) + (event.key === 'ArrowDown' ? 1 : -1)];
    event.preventDefault();
    next?.focus();
    if (next && !compact()) select(next.dataset.version!);
  };

  async function body(id: string) {
    if (id === CURRENT) return currentSource;
    const cached = sources.get(id);
    if (cached !== undefined) return cached;
    const source = await book.noteVersion(noteId, id);
    sources.set(id, source);
    return source;
  }

  async function renderPreview() {
    const token = ++request, id = selected, other = compare.value;
    const version = versions.find(entry => entry.id === id)!;
    $('.history-summary h3').textContent = heading(version);
    summary.textContent = '正在读取…';
    summary.removeAttribute('data-changes');
    errorLine.hidden = true; preview.hidden = true; restore.disabled = compare.disabled = true;
    try {
      const [source, counterpart] = await Promise.all([body(id), body(other || CURRENT)]);
      if (token !== request || !dialog.open) return;
      if (book.root !== root || book.current?.id !== noteId) throw new Error('笔记已切换，请重新打开版本历史');
      const fromOlder = !other || versions.findIndex(entry => entry.id === other) > versions.indexOf(version);
      const lines = fromOlder ? diffVersionLines(counterpart, source) : diffVersionLines(source, counterpart);
      const added = lines.filter(line => line.change === 'added').length, removed = lines.filter(line => line.change === 'removed').length;
      const visible = other ? lines : lines.filter(line => line.change !== 'removed');
      const text = Text.of(visible.map(line => line.text));
      const changed = new Set(visible.flatMap((line, index) => line.change ? [index + 1] : []));
      editorReady ??= import('./rich-editor').then(({ createRichEditor }) => {
        if (!dialog.open) throw new Error('版本历史已关闭');
        return createRichEditor(preview, () => {}, { readOnly: true });
      });
      const editor = await editorReady;
      if (token !== request || !dialog.open) return;
      preview.hidden = false;
      const richView = EditorView.findFromDOM(preview)!;
      richView.contentDOM.tabIndex = 0;
      const showSource = Boolean(other && changed.size && showSourceComparison(text, changed));
      richView.dom.hidden = showSource;
      sourceView?.destroy(); sourceView = undefined;
      // Keep removed rows and fences readable when a rich widget would replace their source lines.
      if (showSource) {
        sourceView = new EditorView({ parent: preview, doc: text, extensions: [
          EditorState.readOnly.of(true), EditorView.editable.of(false), EditorView.lineWrapping,
          EditorView.editorAttributes.of({ class: 'history-source' }),
          EditorView.contentAttributes.of({ 'aria-label': 'Markdown 源码对比', tabindex: '0' }),
        ] });
      } else editor.activate(text.toString(), noteId, noteId, assetBase(book));
      const view = sourceView ?? richView;
      const ranges = visible.flatMap((line, index) => line.change
        ? [Decoration.line({ class: `history-line-${line.change}` }).range(view.state.doc.line(index + 1).from)] : []);
      const style = EditorView.decorations.of(Decoration.set(ranges));
      view.dispatch({ effects: view === richView && marksInstalled ? marks.reconfigure(style) : StateEffect.appendConfig.of(marks.of(style)) });
      if (view === richView) marksInstalled = true;
      view.scrollDOM.scrollTop = 0;
      summary.toggleAttribute('data-changes', !other && added > 0);
      summary.textContent = other && (added || removed) ? `新增 ${added} 行 · 移除 ${removed} 行`
        : !added && !removed ? (id === CURRENT ? '当前正文' : '正文相同')
          : added ? `${added} 行与当前版本不同` : '所选版本缺少当前正文中的部分内容';
      restore.disabled = id === CURRENT || Boolean(book.current?.readOnly) || restoring;
      restore.hidden = Boolean(book.current?.readOnly);
    } catch (reason) {
      if (token !== request || !dialog.open) return;
      summary.textContent = '无法显示该版本';
      errorLine.textContent = message(reason); errorLine.hidden = false;
    } finally {
      if (token === request) compare.disabled = restoring;
    }
  }

  function select(id: string, show = false) {
    if (restoring) return;
    selected = id;
    if (show) dialog.setAttribute('data-preview', '');
    for (const button of list.querySelectorAll('button')) {
      button.setAttribute('aria-current', String(button.dataset.version === id));
    }
    if (compact() && !dialog.hasAttribute('data-preview')) return;
    const other = compare.value;
    compare.replaceChildren(new Option('对比版本…', ''), ...versions.filter(version => version.id !== id).map(version => new Option(heading(version), version.id)));
    if (other && other !== id) compare.value = other;
    detail.toggleAttribute('data-current', id === CURRENT);
    detail.hidden = false;
    void renderPreview();
    if (show && compact()) back.focus();
  }

  async function load() {
    state.hidden = false; retry.hidden = true; detail.hidden = true; list.replaceChildren();
    state.querySelector('h3')!.textContent = '正在读取历史版本…';
    state.querySelector('p')!.textContent = '';
    try {
      if (book.root !== root || book.current?.id !== noteId) throw new Error('笔记已切换，请重新打开版本历史');
      currentSource = book.current.source; current.updatedAt = book.current.updatedAt;
      const older = await book.noteVersions(noteId);
      if (!dialog.open) return;
      if (!older.length) {
        state.querySelector('h3')!.textContent = '暂无历史版本';
        state.querySelector('p')!.textContent = '编辑笔记后，先前的正文会出现在这里。';
        return;
      }
      versions = [current, ...older];
      let lastDay = '', group = list;
      for (const version of versions) {
        const date = day(version.updatedAt);
        if (date !== lastDay) {
          const label = document.createElement('h3'); label.textContent = date;
          group = document.createElement('div'); group.className = 'history-day';
          list.append(label, group); lastDay = date;
        }
        const row = document.createElement('button'); row.type = 'button'; row.dataset.version = version.id;
        row.textContent = version.id === CURRENT ? '当前版本' : time.format(version.updatedAt);
        if (version.id === CURRENT) {
          const updated = document.createElement('small');
          updated.textContent = time.format(version.updatedAt); row.append(updated);
        }
        const disclosure = document.createElement('span'); disclosure.className = 'history-disclosure';
        disclosure.ariaHidden = 'true'; disclosure.textContent = '›'; row.append(disclosure);
        row.onclick = () => select(version.id, true); group.append(row);
      }
      state.hidden = true;
      dialog.setAttribute('data-versions', '');
      select(older[0].id);
    } catch (reason) {
      if (!dialog.open) return;
      state.querySelector('h3')!.textContent = '无法读取历史版本';
      state.querySelector('p')!.textContent = message(reason); retry.hidden = false;
    }
  }
  document.body.append(dialog);
  updateLayout();
  narrow.addEventListener('change', updateLayout);
  dialog.showModal();
  void load();
}
