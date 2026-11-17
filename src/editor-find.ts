import { CharCategory, findClusterBreak, type EditorState } from '@codemirror/state';
import { EditorView, keymap, runScopeHandlers } from '@codemirror/view';
import { closeSearchPanel, findNext, findPrevious, getSearchQuery, openSearchPanel, replaceAll, replaceNext, search, searchKeymap, SearchQuery, setSearchQuery } from '@codemirror/search';
import { popupMenu } from './popup-menu';
import './editor-find.css';

const noWrap = new WeakSet<EditorView>();
function startsWord(_match: string, state: EditorState, from: number) {
  const line = state.doc.lineAt(from);
  const offset = from - line.from;
  if (!offset) return true;
  const previous = line.text.slice(findClusterBreak(line.text, offset, false), offset);
  return state.charCategorizer(from)(previous) !== CharCategory.Word;
}

export function findInEditor(view: EditorView, direction?: 'next' | 'previous') {
  if (!direction) return openSearchPanel(view);
  const query = getSearchQuery(view.state);
  if (noWrap.has(view) && query.valid) {
    const { from, to } = view.state.selection.main;
    const cursor = direction === 'next' ? query.getCursor(view.state, to) : query.getCursor(view.state, 0, from);
    if (cursor.next().done) return true;
  }
  return (direction === 'next' ? findNext : findPrevious)(view);
}

function createFindPanel(view: EditorView) {
  const doc = view.dom.ownerDocument;
  const dom = doc.createElement('div');
  dom.className = 'qingyu-find';
  dom.setAttribute('role', 'search');
  dom.ariaLabel = '在笔记中查找';
  dom.innerHTML = `<div class="qingyu-find-main">
    <div class="qingyu-find-field">
      <button type="button" class="qingyu-find-options" aria-label="查找条件" aria-haspopup="menu" title="查找条件"><svg aria-hidden="true" viewBox="0 0 20 14" fill="none" stroke="currentColor" stroke-width="1.3"><circle cx="6" cy="6" r="3.7"/><path d="m9 9 2.5 2.5m2.1-6 2.2 2.2L18 5.5"/></svg></button>
      <input type="search" placeholder="查找" aria-label="查找" main-field="true" autocomplete="off">
      <output aria-label="匹配数量" aria-live="polite" hidden></output>
      <button type="button" class="qingyu-find-clear" aria-label="清除查找内容" title="清除" hidden>×</button>
    </div>
    <div class="qingyu-find-steps"><button type="button" data-find="previous" aria-label="上一个匹配" title="上一个匹配"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M7 2 3 6l4 4"/></svg></button><button type="button" data-find="next" aria-label="下一个匹配" title="下一个匹配"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m5 2 4 4-4 4"/></svg></button></div>
    <button type="button" class="qingyu-find-done">完成</button>
    <label class="qingyu-find-replace-toggle"><input type="checkbox">替换</label>
  </div>
  <div class="qingyu-find-replace" hidden>
    <input type="text" placeholder="替换" aria-label="替换为" autocomplete="off">
    <div class="qingyu-find-steps"><button type="button" data-replace="one">替换</button><button type="button" data-replace="all">全部</button></div>
    <button type="button" class="qingyu-find-done">完成</button>
  </div>`;
  const input = dom.querySelector<HTMLInputElement>('[main-field]')!;
  const substitution = dom.querySelector<HTMLInputElement>('.qingyu-find-replace input')!;
  const output = dom.querySelector('output')!;
  const clear = dom.querySelector<HTMLButtonElement>('.qingyu-find-clear')!;
  const options = dom.querySelector<HTMLButtonElement>('.qingyu-find-options')!;
  const replaceToggle = dom.querySelector<HTMLInputElement>('[type=checkbox]')!;
  const replaceRow = dom.querySelector<HTMLElement>('.qingyu-find-replace')!;
  const replaceLabel = replaceToggle.closest('label')!;
  const mainDone = dom.querySelector<HTMLButtonElement>('.qingyu-find-main .qingyu-find-done')!;
  const directionButtons = [...dom.querySelectorAll<HTMLButtonElement>('[data-find]')];
  const replaceButtons = [...dom.querySelectorAll<HTMLButtonElement>('[data-replace]')];
  let query = getSearchQuery(view.state);
  let count = 0, first = 0, last = 0;
  let composing = false;

  function updateButtons() {
    dom.dataset.matches = String(count > 0);
    output.hidden = clear.hidden = !query.search;
    output.value = String(count);
    const { from, to } = view.state.selection.main;
    for (const [i, button] of directionButtons.entries()) {
      button.disabled = !count || noWrap.has(view) && (i === 0 ? first >= from : last <= to);
    }
    for (const button of replaceButtons) button.disabled = !count || view.state.readOnly;
    replaceLabel.hidden = view.state.readOnly;
    replaceRow.hidden = view.state.readOnly || !replaceToggle.checked;
    mainDone.hidden = !replaceRow.hidden;
  }

  function countMatches() {
    count = first = last = 0;
    if (query.valid) {
      const matches = query.getCursor(view.state);
      for (let match = matches.next(); !match.done; match = matches.next()) {
        if (!count) first = match.value.from;
        count++;
        last = match.value.to;
      }
    }
    updateButtons();
  }

  function commit(changes: Partial<SearchQuery>, locate = false) {
    const next = new SearchQuery({ ...query, ...changes, literal: true });
    if (next.eq(query)) return;
    const { state } = view;
    let match = locate && next.valid ? next.getCursor(state, state.selection.main.from).next().value : undefined;
    if (!match && locate && next.valid && !noWrap.has(view)) match = next.getCursor(state).next().value;
    const effects = [setSearchQuery.of(next)];
    view.dispatch({ effects, selection: match ? { anchor: match.from, head: match.to } : undefined,
      scrollIntoView: Boolean(match), userEvent: locate ? 'select.search' : undefined });
  }

  input.oninput = event => { if (!composing && !(event as InputEvent).isComposing) commit({ search: input.value }, true); };
  substitution.oninput = event => { if (!composing && !(event as InputEvent).isComposing) commit({ replace: substitution.value }); };
  dom.addEventListener('compositionstart', () => { composing = true; });
  dom.addEventListener('compositionend', () => { composing = false; commit({ search: input.value, replace: substitution.value }, input.value !== query.search); });
  replaceToggle.onchange = updateButtons;
  clear.onclick = () => { commit({ search: '' }); input.focus(); };
  for (const button of dom.querySelectorAll<HTMLButtonElement>('.qingyu-find-done')) button.onclick = () => {
    if (closeSearchPanel(view)) view.focus();
  };
  dom.addEventListener('mousedown', event => {
    const active = view.root.activeElement;
    if ((active === input || active === substitution) && event.target instanceof Element
      && event.target.closest('.qingyu-find-steps, .qingyu-find-replace-toggle')) event.preventDefault();
  });
  directionButtons[0].onclick = () => findInEditor(view, 'previous');
  directionButtons[1].onclick = () => findInEditor(view, 'next');
  replaceButtons[0].onclick = () => { if (!composing) replaceNext(view); };
  replaceButtons[1].onclick = () => { if (!composing) replaceAll(view); };
  options.onclick = () => {
    const bounds = options.getBoundingClientRect();
    const restore = () => { if (input.isConnected) input.focus(); };
    void popupMenu([
      { text: '忽略大小写', checked: !query.caseSensitive, run: () => commit({ caseSensitive: !query.caseSensitive }, true) },
      { text: '循环查找', checked: !noWrap.has(view), run: () => { if (!noWrap.delete(view)) noWrap.add(view); updateButtons(); } },
      { text: '' },
      { text: '包含', checked: !query.wholeWord && !query.test, run: () => commit({ wholeWord: false, test: undefined }, true) },
      { text: '词首匹配', checked: query.test === startsWord, run: () => commit({ wholeWord: false, test: startsWord }, true) },
      { text: '全词匹配', checked: query.wholeWord, run: () => commit({ wholeWord: true, test: undefined }, true) },
      { text: '' },
      { text: '正则表达式', checked: query.regexp, run: () => commit({ regexp: !query.regexp }, true) },
    ], bounds.left, bounds.bottom + 4, console.warn, restore, '查找条件');
  };
  dom.onkeydown = event => {
    if (composing || event.isComposing) { event.stopPropagation(); return; }
    if (runScopeHandlers(view, event, 'search-panel')) { event.preventDefault(); event.stopPropagation(); return; }
    if (event.key !== 'Enter' || event.target !== input && event.target !== substitution) return;
    event.preventDefault();
    if (event.target === substitution) replaceNext(view);
    else findInEditor(view, event.shiftKey ? 'previous' : 'next');
  };
  input.value = query.search;
  substitution.value = query.replace;
  countMatches();
  return {
    dom, top: true,
    mount() { input.focus(); input.select(); },
    update(update: import('@codemirror/view').ViewUpdate) {
      const next = getSearchQuery(update.state);
      const changed = !next.eq(query);
      const recount = changed && (next.search !== query.search || next.regexp !== query.regexp
        || next.caseSensitive !== query.caseSensitive || next.wholeWord !== query.wholeWord || next.test !== query.test);
      if (changed) {
        query = next;
        if (!composing && input.value !== query.search) input.value = query.search;
        if (!composing && substitution.value !== query.replace) substitution.value = query.replace;
        input.setAttribute('aria-invalid', String(Boolean(query.search && !query.valid)));
      }
      if (recount || update.docChanged) countMatches();
      else if (update.selectionSet) updateButtons();
    },
  };
}

export const editorFind = [search({ top: true, literal: true, createPanel: createFindPanel }), keymap.of(searchKeymap.map(binding =>
  binding.run === findNext ? { ...binding, run: view => findInEditor(view, 'next'), shift: view => findInEditor(view, 'previous') } : binding,
))];
