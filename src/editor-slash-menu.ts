import { formatLabel, translatedLabel } from './ui-language';
import { syntaxTree } from '@codemirror/language';
import { EditorSelection, Prec, StateEffect, StateField, type EditorState, type Extension } from '@codemirror/state';
import { EditorView, ViewPlugin, keymap } from '@codemirror/view';
import { createLucideIcon } from '@markra/shared';
import {
  Calendar, Code, FileText, Heading1, Heading2, Heading3, Highlighter, Link, List, ListChecks, ListMinus,
  ListOrdered, MessageSquareText, Minus, Paperclip, Table2, TextAlignStart, TextQuote,
} from 'lucide';
import { markdownCalloutDefinitions, markdownCalloutTypeOrder, type MarkdownCalloutType } from './vendor/markra/shared/markdown-callout';

// 命令的顺序、别名和快捷键照 Pinto 的 SlashCommands.swift。
const specs = [
  { id: 'title', label: '标题', name: 'title', icon: Heading1, aliases: ['h1', 'heading 1'], keys: 'Shift-Mod-T' },
  { id: 'heading', label: '二级标题', name: 'heading', icon: Heading2, aliases: ['h2', 'heading 2'], keys: 'Shift-Mod-H' },
  { id: 'subheading', label: '三级标题', name: 'subheading', icon: Heading3, aliases: ['h3', 'heading 3'], keys: 'Shift-Mod-J' },
  { id: 'body', label: '正文', name: 'body', icon: TextAlignStart, aliases: ['text', 'plain', 'paragraph', 'normal'], keys: 'Shift-Mod-B' },
  { id: 'checklist', label: '任务清单', name: 'checklist', icon: ListChecks, aliases: ['todo', 'to-do', 'task', 'checkbox', 'tick', '待办', '清单'], keys: 'Shift-Mod-L' },
  { id: 'bulleted', label: '项目符号列表', name: 'bulleted list', icon: List, aliases: ['bullet', 'ul', 'unordered', '列表', '无序'], keys: 'Shift-Mod-7' },
  { id: 'dashed', label: '短划线列表', name: 'dashed list', icon: ListMinus, aliases: ['dash'], keys: 'Shift-Mod-8' },
  { id: 'numbered', label: '编号列表', name: 'numbered list', icon: ListOrdered, aliases: ['number', 'ol', 'ordered', '有序'], keys: 'Shift-Mod-9' },
  { id: 'quote', label: '块引用', name: 'block quote', icon: TextQuote, aliases: ['blockquote', '引用'], keys: "Mod-'" },
  {
    id: 'callout', label: '标注', name: 'callout', icon: MessageSquareText,
    aliases: ['note', 'info', 'tip', 'success', 'question', 'warning', 'danger', 'bug', 'example', 'quote', 'error', 'caution', 'hint', 'faq', 'admonition', 'box'],
  },
  { id: 'highlight', label: '高亮', name: 'highlight', icon: Highlighter, aliases: ['marker', 'mark', 'yellow'], keys: 'Shift-Mod-E' },
  { id: 'codeBlock', label: '代码块', name: 'code block', icon: Code, aliases: ['monostyled', 'pre', 'snippet', 'fence'] },
  { id: 'table', label: '表格', name: 'table', icon: Table2, aliases: ['grid', 'columns'], keys: 'Alt-Mod-T' },
  { id: 'divider', label: '分隔线', name: 'divider', icon: Minus, aliases: ['hr', 'line', 'rule', 'separator'] },
  { id: 'date', label: '日期', name: 'date', icon: Calendar, aliases: ['today', 'day', 'now', '今天'], keys: 'Shift-Mod-D' },
  { id: 'link', label: '链接', name: 'link', icon: Link, aliases: ['url', 'web'], keys: 'Mod-K' },
  { id: 'noteLink', label: '链接到笔记', name: 'link to note', icon: FileText, aliases: ['wiki', 'page', 'mention', 'backlink', '[[', '笔记'] },
  { id: 'attach', label: '添加文件', name: 'attach file', icon: Paperclip, aliases: ['file', 'image', 'photo', 'upload', '附件', '图片'], keys: 'Shift-Mod-A' },
] as const;

export type SlashCommand = typeof specs[number]['id'];
export const slashCommands: readonly SlashCommand[] = specs.map(spec => spec.id);
const specOf = (id: SlashCommand) => specs.find(spec => spec.id === id)!;

const maxQuery = 24;
const recentKey = 'slashRecent';
const recentLimit = 3;

function inOrder(query: string, name: string) {
  if (name[0] !== query[0]) return false;
  let at = 0;
  for (const letter of query) {
    at = name.indexOf(letter, at) + 1;
    if (!at) return false;
  }
  return true;
}

/** 名称开头最优先，其次是名称里某个词的开头，再次是别名；都没有时按字母顺序模糊匹配。 */
export function matchSlashCommands(typed: string, commands: readonly SlashCommand[] = slashCommands, recent: readonly SlashCommand[] = []) {
  const query = typed.toLowerCase().trim();
  const rank = (id: SlashCommand) => { const index = recent.indexOf(id); return index < 0 ? recent.length : index; };
  const ordered = (list: { id: SlashCommand; score: number; index: number }[]) => list
    .sort((a, b) => a.score - b.score || rank(a.id) - rank(b.id) || a.index - b.index).map(item => item.id);
  if (!query) return ordered(commands.map((id, index) => ({ id, score: 0, index })));
  const score = (id: SlashCommand) => {
    const { label, name, aliases } = specOf(id);
    if (label.startsWith(query) || name.startsWith(query)) return 0;
    if (name.split(' ').some(word => word.startsWith(query)) || label.includes(query)) return 1;
    return aliases.some(alias => alias.startsWith(query)) ? 2 : -1;
  };
  const found = commands.map((id, index) => ({ id, score: score(id), index })).filter(item => item.score >= 0);
  if (found.length || query.length < 2) return ordered(found);
  return ordered(commands.map((id, index) => ({ id, score: 0, index })).filter(item => inOrder(query, specOf(item.id).name)));
}

/** `/warn` 这样的输入直接指定标注类型；输入的是 callout 本身时不指定。 */
export function slashCalloutType(typed: string): MarkdownCalloutType | null {
  const query = typed.toLowerCase().trim();
  if (query.length < 2 || 'callout'.startsWith(query) || query.startsWith('callout')) return null;
  const named = markdownCalloutTypeOrder.find(type => type.startsWith(query));
  if (named) return named;
  const other = ({ error: 'danger', caution: 'warning', hint: 'tip', faq: 'question' } as Record<string, MarkdownCalloutType>)[query];
  return other ?? null;
}

/** 光标前正在输入的 `/查询`。斜杠必须在行首或空格之后，代码、表格行里不算。 */
export function slashQueryAt(state: EditorState) {
  if (state.readOnly || state.selection.ranges.length !== 1 || !state.selection.main.empty) return null;
  const caret = state.selection.main.head;
  const line = state.doc.lineAt(caret);
  const before = line.text.slice(Math.max(0, caret - line.from - maxQuery - 1), caret - line.from);
  const offset = before.lastIndexOf('/');
  if (offset < 0) return null;
  const from = caret - before.length + offset;
  const query = state.sliceDoc(from + 1, caret);
  if (query.startsWith(' ') || query.includes('\t')) return null;
  const head = state.sliceDoc(line.from, from);
  if (head && !/[ \t]$/u.test(head)) return null;
  if (head.trimStart().startsWith('|') || (head.split('`').length - 1) % 2) return null;
  for (let node: { name: string; parent: unknown } | null = syntaxTree(state).resolve(from, -1); node; node = node.parent as typeof node) {
    if (node.name === 'FencedCode' || node.name === 'CodeBlock') return null;
  }
  return { from, to: caret, query };
}

function recentCommands(): SlashCommand[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(recentKey) ?? '[]');
    return Array.isArray(stored) ? stored.filter((id): id is SlashCommand => slashCommands.includes(id as SlashCommand)) : [];
  } catch { return []; }
}

function rememberCommand(id: SlashCommand) {
  try {
    localStorage.setItem(recentKey, JSON.stringify([id, ...recentCommands().filter(other => other !== id)].slice(0, recentLimit)));
  } catch { /* 存不下就不记，菜单照常用 */ }
}

type MenuState = {
  from: number; to: number; query: string; commands: SlashCommand[]; recent: number;
  index: number; callout: MarkdownCalloutType | null;
};
type FieldValue = { menu: MenuState | null; dismissed: number | null };
const menuEffect = StateEffect.define<{ close: true } | { select: number }>();

function slashField(available: () => readonly SlashCommand[]) {
  return StateField.define<FieldValue>({
    create: () => ({ menu: null, dismissed: null }),
    update(previous, transaction) {
      const effect = transaction.effects.find(item => item.is(menuEffect))?.value;
      if (effect && 'close' in effect) return { menu: null, dismissed: previous.menu?.from ?? previous.dismissed };
      // 光标没动、内容没变时不用重算。
      if (!effect && !transaction.docChanged && !transaction.selection) return previous;
      const range = slashQueryAt(transaction.state);
      if (!range) return { menu: null, dismissed: null };
      const dismissed = previous.dismissed === null ? null : transaction.changes.mapPos(previous.dismissed, 1);
      if (range.from === dismissed) return { menu: null, dismissed };
      const recent = recentCommands();
      const commands = matchSlashCommands(range.query, available(), recent);
      if (!commands.length) return { menu: null, dismissed: null };
      const same = previous.menu && commands.join() === previous.menu.commands.join();
      return {
        dismissed: null,
        menu: {
          ...range, commands,
          recent: range.query ? 0 : recent.filter(id => commands.includes(id)).length,
          index: effect && 'select' in effect ? effect.select : same ? previous.menu!.index : 0,
          callout: slashCalloutType(range.query),
        },
      };
    },
  });
}

const apple = /Mac|iPhone|iPad/u.test(globalThis.navigator?.platform ?? '');
function shortcutText(keys: string) {
  const parts = keys.split('-');
  const key = parts.pop()!;
  if (!apple) return [...parts.map(part => ({ Shift: 'Shift', Mod: 'Ctrl', Alt: 'Alt' })[part]).sort((a, b) => Number(a !== 'Ctrl') - Number(b !== 'Ctrl')), key].join('+');
  return parts.map(part => ({ Shift: '⇧', Mod: '⌘', Alt: '⌥' })[part]).join('') + key;
}

export type SlashMenuOptions = {
  /** 手机上菜单放进键盘上方的工具条。 */
  host?: HTMLElement;
  /** 执行命令。查询文字已经删掉，光标在原来斜杠的位置。 */
  run(command: SlashCommand, callout: MarkdownCalloutType | null): void;
  /** 能不能从菜单添加文件（手机在键盘条上有回形针，不重复提供）。 */
  canAttach?: () => boolean;
};

export function slashMenu({ host, run, canAttach }: SlashMenuOptions): Extension {
  const field = slashField(() => canAttach?.() ? slashCommands : slashCommands.filter(id => id !== 'attach'));
  const menuOf = (view: EditorView) => view.state.field(field).menu;

  function runCommand(view: EditorView, id: SlashCommand) {
    const menu = menuOf(view);
    if (!menu) return false;
    view.dispatch({
      changes: { from: menu.from, to: menu.to }, selection: EditorSelection.cursor(menu.from),
      effects: menuEffect.of({ close: true }), userEvent: 'input.delete',
    });
    rememberCommand(id);
    run(id, id === 'callout' ? menu.callout : null);
    return true;
  }
  const move = (step: number) => (view: EditorView) => {
    const menu = menuOf(view);
    if (!menu) return false;
    const count = menu.commands.length;
    view.dispatch({ effects: menuEffect.of({ select: (menu.index + step + count) % count }) });
    return true;
  };
  const choose = (view: EditorView) => {
    const menu = menuOf(view);
    return menu ? runCommand(view, menu.commands[menu.index]) : false;
  };

  const panelView = ViewPlugin.fromClass(class {
    private panel = document.createElement('div');
    private signature = '';
    private readonly shell: HTMLElement | null;
    private readonly shellObserver?: MutationObserver;

    constructor(private view: EditorView) {
      this.panel.className = 'editor-slash-menu';
      this.panel.setAttribute('role', 'listbox');
      this.panel.ariaLabel = translatedLabel('格式菜单');
      this.panel.hidden = true;
      (host ?? view.dom).append(this.panel);
      const shell = this.shell = host ? null : view.dom.closest<HTMLElement>('#app-shell');
      if (shell) {
        this.shellObserver = new MutationObserver(() => {
          if (!shell.hasAttribute('inert') && !view.hasFocus) this.panel.hidden = true;
        });
        this.shellObserver.observe(shell, { attributeFilter: ['inert'] });
      }
    }

    update() {
      const { view, panel } = this;
      if (host) view.requestMeasure({
        key: host,
        read: () => {
          const selected = host.querySelector<HTMLElement>('.note-link-completion [aria-selected=true], .editor-slash-menu:not([hidden]) [aria-selected=true]');
          const row = selected?.parentElement;
          if (!row?.clientWidth || !selected) return null;
          const option = selected.getBoundingClientRect(), bounds = row.getBoundingClientRect();
          const offset = option.left < bounds.left ? option.left - bounds.left : Math.max(0, option.right - bounds.right);
          return { row, offset };
        },
        write: result => { if (result?.offset) result.row.scrollLeft += result.offset; },
      });
      const menu = menuOf(view);
      if (!menu || view.compositionStarted) {
        panel.hidden = true;
        return;
      }
      if (!view.hasFocus) {
        // Reconciliation can briefly drop focus before restoring the writing caret.
        if (!this.shell?.hasAttribute('inert') || document.activeElement !== document.body) panel.hidden = true;
        return;
      }
      const signature = `${menu.commands.join()}|${menu.index}|${menu.recent}|${menu.callout}`;
      if (signature !== this.signature) {
        this.signature = signature;
        const count = menu.commands.length;
        panel.replaceChildren(...menu.commands.flatMap((id, index) => {
          const spec = specOf(id);
          const named = id === 'callout' && menu.callout ? `${markdownCalloutDefinitions[menu.callout].label} 标注` : spec.label;
          const button = document.createElement('button');
          button.type = 'button';
          button.tabIndex = -1;
          button.dataset.command = id;
          button.setAttribute('role', 'option');
          button.ariaSelected = String(index === menu.index);
          button.ariaLabel = formatLabel('{name}，第 {index} 项，共 {count} 项', { name: named, index: index + 1, count });
          const label = document.createElement('span');
          label.className = 'slash-command-label';
          label.textContent = named;
          button.append(createLucideIcon(document, spec.icon, 'slash-command-icon'), label);
          if ('keys' in spec) {
            const keys = document.createElement('kbd');
            keys.textContent = shortcutText(spec.keys);
            button.append(keys);
          }
          button.addEventListener('pointerdown', event => { event.preventDefault(); });
          button.onclick = () => { runCommand(view, id); };
          if (index !== menu.recent - 1 || index === count - 1) return [button];
          // 上面是最近用过的，下面是菜单原本的顺序。
          const line = document.createElement('div');
          line.className = 'slash-recent-line';
          line.ariaHidden = 'true';
          return [button, line];
        }));
        // 列表比九行半长时，底下半行渐隐，提示还能往下滚。
        panel.toggleAttribute('data-more', count > 9.5 && menu.index < count - 2);
      }
      if (host) { panel.hidden = false; return; }
      const position = menu.to;
      view.requestMeasure({
        key: panel,
        read: editor => ({ caret: editor.coordsAtPos(position), bounds: editor.scrollDOM.getBoundingClientRect() }),
        write: ({ caret, bounds }) => {
          if (!view.hasFocus || view.state.field(field, false)?.menu?.to !== position || view.compositionStarted) return;
          panel.hidden = !caret || caret.bottom < bounds.top || caret.top > bounds.bottom;
          if (!caret || panel.hidden) return;
          const below = window.innerHeight - caret.bottom - 6;
          const above = caret.top - 6;
          const opensAbove = below < 132 && above > below;
          const left = Math.min(caret.left, window.innerWidth - panel.offsetWidth - 6);
          panel.style.left = `${Math.max(6, left)}px`;
          panel.style.maxHeight = `${Math.min(panel.querySelector('.slash-recent-line') ? 245 : 236, opensAbove ? above : below)}px`;
          panel.style.top = `${opensAbove ? caret.top - panel.offsetHeight - 4 : caret.bottom + 4}px`;
          const selected = panel.querySelector<HTMLElement>('[aria-selected=true]');
          if (selected && selected.offsetTop < panel.scrollTop + 4) panel.scrollTop = selected.offsetTop - 4;
          else if (selected && selected.offsetTop + selected.offsetHeight > panel.scrollTop + panel.clientHeight - 4) {
            panel.scrollTop = selected.offsetTop + selected.offsetHeight - panel.clientHeight + 4;
          }
        },
      });
    }

    destroy() { this.shellObserver?.disconnect(); this.panel.remove(); }
  });

  return [
    field,
    Prec.highest(keymap.of([
      { key: 'ArrowDown', run: move(1) },
      { key: 'ArrowUp', run: move(-1) },
      { key: 'Enter', run: choose },
      { key: 'Tab', run: choose },
      { key: 'Escape', run: view => { if (!menuOf(view)) return false; view.dispatch({ effects: menuEffect.of({ close: true }) }); return true; } },
    ])),
    panelView,
  ];
}
