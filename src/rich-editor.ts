import { defaultKeymap, historyKeymap, isolateHistory } from '@codemirror/commands';
import { autocompletion, closeCompletion } from '@codemirror/autocomplete';
import { syntaxTree } from '@codemirror/language';
import { EditorSelection, EditorState, Transaction, type ChangeSet } from '@codemirror/state';
import { EditorView, drawSelection, keymap, placeholder, tooltips } from '@codemirror/view';
import { invoke } from '@tauri-apps/api/core';
import {
  blocksPlugin, calloutPreviewPlugin, codeBlockPreviewPlugin, footnotePreviewPlugin,
  formattingPlugin, frontmatterPreviewPlugin, horizontalRulePlugin, imagePreviewPlugin,
  linksPlugin, liveMarkdown, markdownEditingPlugin, mathPreviewPlugin, rawHtmlPreviewPlugin,
  resolveSafeImageSource, resolveSafeLinkTarget, runMarkraCommand,
  tablePreviewPlugin, trailingSpacePlugin,
} from '@markra/editor/codemirror';
import { markdownCalloutDefinitions, markdownCalloutTypeOrder } from '@markra/shared';
import { createCalloutIcon } from './vendor/markra/editor/codemirror/callout-preview';
import { fencedCodeTextAt } from './vendor/markra/editor/codemirror/code-block';
import { imageMenuTarget } from './vendor/markra/editor/codemirror/image';
import { captureActiveTableMenu, captureTableFormat, captureTableInsertion, formatActiveTableSelection, restoreCopiedTableHeader, tableContextMenu } from './vendor/markra/editor/codemirror/table';
import { formatPanelState, formatPanelTransaction, formatTransaction, paragraphFormat, replaceSource, untickChecklist, type Format, type PanelFormat } from './format';
import { isLocalAsset } from './server-assets';
import { popupMenu, popupMenuOpen, type MenuItem } from './popup-menu';
import { renderEditorDiagram } from './editor-diagrams';
import { linkCards, linkFormAt, linkStyleSource, otherLinkStyles, refreshLinkCards, type LinkPreviewLoader, type NoteCardPreview } from './editor-link-cards';
import { slashMenu, type SlashCommand } from './editor-slash-menu';
import { changeTint, markChanges } from './editor-change-tint';
import { showTablePanel } from './editor-table-panel';
import { editorHistoryMenuUpdates } from './editor-history-menu';
import { closeNoteDatePicker, noteDateChips } from './editor-note-dates';
import { editorFind, findInEditor } from './editor-find';
import { foldAllHeadings, headingFolds, reloadHeadingFolds, revealHeadingSection } from './editor-heading-folds';
import { wikiLinkAvailability, wikiLinksChanged, wikiLinkTarget } from './vendor/markra/editor/codemirror/wiki-link';
import { noteHeadingOffset } from './note-links';
import type { PictureRange } from './note-picture';
import { checklistSorting } from './editor-checklist';
import type { createNoteRelationsFooter } from './editor-note-relations';
import 'katex/dist/katex.min.css';
import './rich-editor.css';

type NoteLinkSuggestion = { title: string; target: string; detail?: string };
type EditorOptions = {
  readOnly?: boolean;
  sortCheckedItems?: () => boolean;
  openAsset?: (noteId: string, path: string) => void;
  openNoteLink?: (target: string) => void;
  noteLinkExists?: (target: string) => boolean;
  describeNoteLink?: (target: string) => NoteCardPreview | undefined;
  loadLinkPreview?: LinkPreviewLoader;
  onError?: (message: string) => void;
  suggestNoteLinks?: (query: string, draft?: string) => NoteLinkSuggestion[] | Promise<NoteLinkSuggestion[]>;
  sharePicture?: (source: string, selection: PictureRange) => void;
  relations?: ReturnType<typeof createNoteRelationsFooter>;
  suggestionsHost?: HTMLElement;
  /** 从斜杠菜单添加文件；不传则菜单里没有这一项。 */
  attachFile?: () => void;
  selectionChanged?: () => void;
};

const commands = {
  bold: 'format.bold', italic: 'format.italic', strike: 'format.strikethrough',
  highlight: 'format.highlight', code: 'format.code', callout: 'block.callout', table: 'block.table',
} as const;
const slashFormats: Partial<Record<SlashCommand, Format>> = {
  checklist: 'task', bulleted: 'bullet', dashed: 'list', numbered: 'ordered', codeBlock: 'codeblock',
};
const linkStyles = { link: '显示为链接', compact: '紧凑显示', card: '显示为卡片' } as const;
const imageSizes = { small: '小图', fit: '适应宽度', cover: '封面' } as const;

export function createRichEditor(parent: HTMLElement, onChange: (changes: ChangeSet) => void, options: EditorOptions = {}) {
  const { suggestNoteLinks, noteLinkExists, suggestionsHost } = options;
  let active = false;
  let documentId: string | undefined, noteId: string | undefined, assetBase: string | undefined;
  let captured: EditorSelection | undefined, pending: ChangeSet | undefined;
  let closeTablePanel: (() => void) | undefined;
  let readOnly = Boolean(options.readOnly);
  const checklist = checklistSorting(() => active && (options.sortCheckedItems?.() ?? true));
  parent.replaceChildren();
  parent.dataset.readOnly = String(readOnly);

  function resolveImage(source: string) {
    if (!noteId || !isLocalAsset(source) || source.startsWith('/') && !/^\/(i|files)\//.test(source)) return resolveSafeImageSource(source);
    try {
      const path = decodeURIComponent(source.split(/[?#]/)[0]);
      const base = assetBase ?? `/api/web/assets/${encodeURIComponent(noteId)}`;
      return `${base}${base.includes('?') ? '&' : '?'}${new URLSearchParams({ path })}`;
    } catch { return null; }
  }
  const onMac = document.documentElement.dataset.platform === 'macos' && !document.documentElement.hasAttribute('data-mobile');
  function copyText(text: string) {
    return onMac ? invoke<void>('table_clipboard', { text }) : navigator.clipboard.writeText(text);
  }
  const external = /^(https?:|mailto:|tel:)/i;
  function openLink(target: string) {
    const noteTarget = wikiLinkTarget(target);
    if (noteTarget) options.openNoteLink?.(noteTarget);
    else if (target.startsWith('#')) {
      let heading: string;
      try { heading = decodeURIComponent(target.slice(1)); } catch { return; }
      const doc = view.state.doc;
      void noteHeadingOffset(doc.toString(), heading).then(position => {
        if (active && view.state.doc === doc && position !== undefined) scrollToLine(doc.lineAt(position).number);
      });
    } else if (noteId && isLocalAsset(target)) options.openAsset?.(noteId, target);
    else if (external.test(target)) {
      if (onMac) void invoke('open_external_link', { target }).catch(error => options.onError?.(String(error)));
      else window.open(target, '_blank', 'noopener,noreferrer');
    }
  }
  const images = { resolveSource: ({ source }: { source: string }) => resolveImage(source) };
  const resolveLink = (source: string) => wikiLinkTarget(source) ? source : resolveSafeLinkTarget(source);
  const links = {
    activation: 'click' as const,
    resolveTarget: ({ source }: { source: string }) => resolveLink(source),
    open: ({ target }: { target: string }) => openLink(target),
  };
  function textMenu(editor: EditorView, visibleSelection: boolean): MenuItem[] {
    const selected = visibleSelection || editor.state.selection.ranges.some(range => !range.empty);
    return [
      { text: '剪切', enabled: selected, nativeRole: 'Cut' as const, run: () => { document.execCommand('cut'); } },
      { text: '复制', enabled: selected, nativeRole: 'Copy' as const, run: () => { document.execCommand('copy'); } },
      { text: '粘贴', nativeRole: 'Paste' as const, run: async () => {
        const before = editor.state;
        const text = await navigator.clipboard.readText();
        if (text && editor.state === before && editor.hasFocus && !editor.compositionStarted) {
          const data = new DataTransfer();
          data.setData('text/plain', text);
          editor.contentDOM.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
        }
      } },
    ];
  }
  function pictureSelection(editor: EditorView): PictureRange | undefined {
    const dom = editor.contentDOM.ownerDocument.getSelection();
    const range = dom?.rangeCount ? dom.getRangeAt(0) : undefined;
    const inside = range && editor.contentDOM.contains(range.startContainer) && editor.contentDOM.contains(range.endContainer);
    const element = range?.commonAncestorContainer instanceof Element ? range.commonAncestorContainer : range?.commonAncestorContainer.parentElement;
    const table = inside && element?.closest('.cm-markra-table');
    if (table && range) {
      if (range.collapsed || !range.toString().trim()) return;
      const at = editor.posAtDOM(table);
      if (at < editor.state.doc.length) return { from: at, to: at + 1 };
    }
    const selection = editor.state.selection.main;
    if (!selection.empty && editor.state.sliceDoc(selection.from, selection.to).trim()) return selection;
    if (!inside || !range || range.collapsed || !range.toString().trim()) return;
    const from = editor.posAtDOM(range.startContainer, range.startOffset), to = editor.posAtDOM(range.endContainer, range.endOffset);
    if (from < to) return { from, to };
  }
  function pictureMenu(editor: EditorView, selected: PictureRange | undefined) {
    if (!selected || !options.sharePicture) return [];
    const doc = editor.state.doc;
    return [{ text: '分享为图片…', run: () => { if (editor.state.doc === doc) options.sharePicture?.(doc.toString(), selected); } }, { text: '' }];
  }
  function flush() {
    if (!pending || view.compositionStarted) return;
    const changes = pending; pending = undefined;
    onChange(changes);
  }
  const extensions = () => [
    noteLinkExists ? wikiLinkAvailability.of(noteLinkExists) : [],
    EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly),
    EditorView.lineWrapping, drawSelection(),
    // 对应 Pinto 的 titleHintText：空笔记里提示第一行就是标题，只在光标在里面时显示（样式见 rich-editor.css）。
    readOnly ? [] : placeholder('标题'),
    EditorView.contentAttributes.of({ 'aria-label': readOnly ? 'Markdown 预览' : '所见即所得编辑区', spellcheck: 'false', autocapitalize: 'off' }),
    editorFind,
    EditorView.scrollMargins.of(view => ({
      top: parseFloat(getComputedStyle(view.scrollDOM).scrollPaddingTop) || 0,
    })),
    !readOnly && suggestionsHost ? [
      tooltips({ parent: suggestionsHost, position: 'absolute' }),
      EditorState.phrases.of({ Completions: '链接候选' }),
      EditorView.domEventHandlers({
        compositionstart(_event, editor) {
          suggestionsHost.setAttribute('data-composing', '');
          closeCompletion(editor);
        },
        compositionend() { suggestionsHost.removeAttribute('data-composing'); },
        blur() { suggestionsHost.removeAttribute('data-composing'); },
      }),
    ] : [],
    readOnly ? [] : [checklist.extension, keymap.of([...defaultKeymap, ...historyKeymap])],
    !readOnly && suggestNoteLinks ? autocompletion({
      activateOnTypingDelay: 0,
      interactionDelay: 0,
      maxRenderedOptions: 8,
      closeOnBlur: true,
      tooltipClass: () => 'note-link-completion',
      override: [async context => {
        const match = context.matchBefore(/\[\[[^\[\]\r\n|]*$/u);
        if (!match || match.from > 0 && context.state.doc.sliceString(match.from - 1, match.from) === '\\') return null;
        const query = match.text.slice(2);
        const hasHeading = query.includes('#');
        let to = context.pos, closing = ']]';
        const start = syntaxTree(context.state).resolveInner(match.from, 1);
        for (let node: typeof start | null = start; node; node = node.parent) {
          if (['InlineCode', 'CodeBlock', 'FencedCode'].includes(node.name)) return null;
          if (node.name === 'WikiLink' || node.name === 'WikiCard') {
            if (!hasHeading) return null;
            const tail = context.state.sliceDoc(context.pos, node.to);
            const alias = tail.indexOf('|');
            if (alias >= 0) closing = tail.slice(alias);
            to = node.to;
          }
        }
        const choices = await suggestNoteLinks(query, hasHeading ? context.state.doc.toString() : undefined);
        if (context.aborted || !choices.length) return null;
        return {
          from: match.from,
          to,
          filter: false,
          options: choices.map(({ title, target, detail }) => ({
            label: title, detail, apply: `[[${target}${closing}`,
            displayLabel: suggestionsHost && hasHeading && target.includes('#') ? `# ${title}` : undefined,
          })),
        };
      }],
    }) : [],
    readOnly ? [] : tableContextMenu.of(({ x, y, bounds, cell, tableSelection }, commands, scope, restoreFocus) => {
      const choice = commands.choices;
      if (choice) {
        const items = choice.options.map(value => ({
          text: value, checked: choice.value.toLowerCase() === value.toLowerCase(), run: () => choice.set(value),
        }));
        void popupMenu([...items, { text: '' }, { text: '清空', run: () => choice.set('') }],
          x, y, message => options.onError?.(String(message)), restoreFocus, '选择单元格值');
        return;
      }
      const command = (text: string, run?: () => void, symbol?: string, destructive = false) => ({ text, run, enabled: Boolean(run), symbol, destructive });
      const below = command('在下方插入行', commands.insertRowAfter, 'arrow.down'), above = command('在上方插入行', commands.insertRowBefore, 'arrow.up');
      const after = command('在右侧插入列', commands.insertColumnAfter, 'arrow.right'), before = command('在左侧插入列', commands.insertColumnBefore, 'arrow.left');
      const deleteRow = command('删除行', commands.deleteRow, 'trash', true), deleteColumn = command('删除列', commands.deleteColumn, 'trash', true);
      const separator = { text: '' };
      const deleteTable = command('删除表格', commands.deleteTable, 'trash', true);
      if (scope === 'table' && document.documentElement.hasAttribute('data-mobile')) {
        closeTablePanel?.();
        closeTablePanel = showTablePanel(cell, [[above, below, deleteRow], [before, after, deleteColumn]], deleteTable, restoreFocus, () => { closeTablePanel = undefined; }, view.scrollDOM);
        return;
      }
      const structure = [below, above, after, before, separator, deleteRow, deleteColumn, separator, deleteTable];
      const { text, replace } = commands.clipboard;
      const write = () => copyText(text);
      const paste = async () => {
        const value = onMac ? await invoke<string | null>('table_clipboard') : await navigator.clipboard.readText();
        if (value) replace(value);
      };
      const cellItems = (canPaste: boolean): MenuItem[] => [
        ...pictureMenu(view, tableSelection),
        { text: '剪切', enabled: Boolean(text), needsUserActivation: !onMac, run: async () => { await write(); replace(''); } },
        { text: '复制', enabled: Boolean(text), needsUserActivation: !onMac, run: write },
        { text: '粘贴', enabled: canPaste, needsUserActivation: !onMac, run: paste },
        separator, ...structure,
      ];
      const lineSections = scope === 'row' ? [[above, below], [deleteRow], [deleteTable]] : scope === 'column' ? [[before, after], [deleteColumn], [deleteTable]] : undefined;
      const items = lineSections ? lineSections.flatMap((section, index) => index ? [separator, ...section] : section)
        : scope !== 'cell' ? structure
        : onMac ? invoke<boolean>('table_clipboard_has_text').then(cellItems, () => cellItems(false))
        : cellItems(typeof navigator.clipboard?.readText === 'function');
      const label = scope === 'row' ? '行选项' : scope === 'column' ? '列选项' : '表格操作';
      const showWeb = () => popupMenu(items, x, y, message => options.onError?.(message), restoreFocus, label).then(() => {
        if ((scope === 'row' || scope === 'column') && !popupMenuOpen()) restoreFocus();
      });
      if (bounds && lineSections && '__TAURI_INTERNALS__' in window
        && document.documentElement.hasAttribute('data-mobile') && !/Android/i.test(navigator.userAgent)) {
        const viewport = window.visualViewport;
        const scale = viewport?.scale ?? 1;
        const frame = [(bounds.left - (viewport?.offsetLeft ?? 0)) * scale, (bounds.top - (viewport?.offsetTop ?? 0)) * scale,
          bounds.width * scale, bounds.height * scale];
        const enabled = lineSections.map(section => section.filter(item => item.enabled));
        const sections = enabled.map(section => section.map(({ text, symbol, destructive }) => ({ text, symbol, destructive })));
        const actions = enabled.flat();
        void invoke<number | null>('show_table_handle_menu', { menu: { frame, sections } }).then(index => {
          restoreFocus();
          if (index !== null) actions[index]?.run?.();
        }, showWeb);
      } else void showWeb();
    }),
    liveMarkdown({
      plugins: [
        blocksPlugin({
          keybindings: false, headingLevelControl: false, labels: {
            'block.paragraph': '正文', 'block.heading.1': '标题', 'block.heading.2': '二级标题',
            'block.heading.3': '三级标题', 'block.heading.4': '四级标题', 'block.heading.5': '五级标题',
            'block.heading.6': '六级标题', 'block.bullet-list': '项目符号列表', 'block.task-list': '任务清单',
            'block.ordered-list': '编号列表', 'block.quote': '引用', 'block.callout': '标注',
            'block.code': '代码块', 'block.table': '表格',
          },
        }),
        calloutPreviewPlugin({
          showTypeMenu(anchor, current, apply, restoreFocus) {
            const bounds = anchor.getBoundingClientRect();
            void popupMenu(markdownCalloutTypeOrder.map(type => ({
              text: markdownCalloutDefinitions[type].label,
              icon: createCalloutIcon(anchor.ownerDocument, type),
              checked: type === current,
              run: () => apply(type),
            })), bounds.left, bounds.bottom + 4, message => options.onError?.(message), restoreFocus, '标注类型');
          },
        }),
        codeBlockPreviewPlugin({
          showLineNumbers: false,
          labels: { mermaidDiagram: 'Mermaid 图表', mermaidError: '无法渲染 Mermaid 图表' },
          renderMermaid: renderEditorDiagram,
        }), footnotePreviewPlugin(), frontmatterPreviewPlugin(),
        formattingPlugin({ keybindings: false }), horizontalRulePlugin(), imagePreviewPlugin(images), linksPlugin(links),
        mathPreviewPlugin(), markdownEditingPlugin(), rawHtmlPreviewPlugin({ resolveImageSrc: source => resolveImage(source) ?? '' }),
        tablePreviewPlugin({ images, links }), trailingSpacePlugin(),
      ],
    }),
    linkCards(openLink, options.describeNoteLink, options.loadLinkPreview),
    noteDateChips,
    editorHistoryMenuUpdates,
    headingFolds(() => documentId),
    changeTint,
    readOnly ? [] : slashMenu({
      host: suggestionsHost,
      canAttach: () => Boolean(options.attachFile),
      run(command, callout) {
        const editor = view;
        if (command === 'noteLink') {
          // 插入 [[ 后，笔记标题的补全会在原地打开。
          editor.dispatch(editor.state.replaceSelection('[['), { userEvent: 'input.type', scrollIntoView: true });
        } else if (command === 'attach') options.attachFile?.();
        else if (command === 'callout' && callout) {
          editor.dispatch(editor.state.replaceSelection(`> [!${markdownCalloutDefinitions[callout].marker}]\n> `), { userEvent: 'input.format', scrollIntoView: true });
        } else format(slashFormats[command as keyof typeof slashFormats] ?? command as Format);
        editor.focus();
      },
    }),
    EditorView.updateListener.of(update => {
      if (active && (update.selectionSet || update.docChanged)) options.selectionChanged?.();
      if (!active || readOnly || !update.docChanged) return;
      captured = captured?.map(update.changes);
      if (update.view.compositionStarted || pending) {
        pending = pending ? pending.compose(update.changes) : update.changes;
        if (!update.view.compositionStarted) queueMicrotask(flush);
      } else onChange(update.changes);
    }),
    EditorView.domEventHandlers({
      paste(event, editor) {
        const text = event.clipboardData?.getData('text/plain');
        if (readOnly || !text || editor.compositionStarted) return false;
        const { state } = editor;
        const { from, to } = state.selection.main;
        const start = state.doc.lineAt(from), end = state.doc.lineAt(to);
        const before = from > start.from && state.sliceDoc(start.from, from).trim() ? '\n\n' : '';
        const after = to < end.to && state.sliceDoc(to, end.to).trim() ? '\n\n' : '';
        const repaired = restoreCopiedTableHeader(text, before, after);
        if (repaired === text) return false;
        const startNode = syntaxTree(state).resolveInner(from, -1);
        for (let node: typeof startNode | null = startNode; node; node = node.parent) {
          if (['FencedCode', 'CodeBlock', 'InlineCode'].includes(node.name)) return false;
        }
        editor.dispatch(state.replaceSelection(repaired), { userEvent: 'input.paste', scrollIntoView: true });
        return true;
      },
      compositionend() { setTimeout(flush, 0); },
      blur() { setTimeout(flush, 0); },
      contextmenu(event, editor) {
        if (!(event.target instanceof Element) || editor.compositionStarted) return false;
        const image = imageMenuTarget(event.target);
        if (image) {
          const doc = editor.state.doc;
          const edit = (from: number, to: number, insert = '') => {
            if (editor.state.doc !== doc || editor.compositionStarted) return;
            editor.dispatch({ changes: { from, to, insert }, selection: { anchor: image.line.from },
              userEvent: 'input.image', annotations: isolateHistory.of('full') });
          };
          const items: MenuItem[] = [{ text: '打开图片', run: () => {
            if (editor.state.doc !== doc) return;
            if (noteId && options.openAsset && isLocalAsset(image.source)) openLink(image.source);
            else image.openPreview();
          } }];
          if (!readOnly) items.push({ text: '图片尺寸', items: Object.entries(imageSizes).map(([size, text]) => ({
            text, checked: size === image.size,
            run: () => { if (size !== image.size) edit(image.sizeFrom, image.sizeTo, size === 'fit' ? '' : `|${size}`); },
          })) }, { text: '' }, {
            text: '从笔记中移除', destructive: true,
            run: () => edit(image.line.from, Math.min(image.line.to + 1, doc.length)),
          });
          void popupMenu(items, event.clientX, event.clientY, message => options.onError?.(message), () => editor.focus(), '图片操作');
          return true;
        }
        const anchor = event.target.closest<HTMLAnchorElement>('a[href]');
        const embed = anchor?.classList.contains('qingyu-link-card');
        const line = embed ? anchor : event.target.closest('.cm-line');
        const before = editor.state.doc;
        const position = line ? editor.posAtDOM(line) : undefined;
        const standalone = position !== undefined ? linkFormAt(editor.state, position) : null;
        const target = standalone?.form.href ?? (anchor && resolveLink(anchor.getAttribute('href') ?? ''));
        const canUntick = !readOnly && position !== undefined && untickChecklist(editor.state, position);
        const selected = readOnly ? undefined : pictureSelection(editor);
        const picture = pictureMenu(editor, selected);
        const codeFrom = event.target.closest<HTMLElement>('.cm-markra-code-line')?.dataset.codeBlockFrom;
        const code = codeFrom === undefined ? undefined : fencedCodeTextAt(editor.state, Number(codeFrom));
        if (!canUntick && !target && !picture.length && !code) return false;
        const clipboard = readOnly || embed ? [] : textMenu(editor, Boolean(selected));
        const restyle = !readOnly && standalone ? otherLinkStyles(standalone.form).map(style => ({
          text: linkStyles[style], run: () => rewrite(linkStyleSource(standalone.form, style, anchor?.dataset.webTitle)),
        })) : [];
        function rewrite(text?: string) {
          if (!standalone || editor.state.doc !== before || editor.compositionStarted) return;
          const { line } = standalone;
          const to = text === undefined && line.to < before.length ? line.to + 1 : line.to;
          editor.dispatch({ changes: { from: line.from, to, insert: text ?? '' }, selection: { anchor: line.from },
            userEvent: 'input.link', annotations: isolateHistory.of('full') });
        }
        const menuItems = (alternatives: MenuItem[] = []): MenuItem[] => {
          const items: MenuItem[] = [...picture];
          if (code) items.push({ text: '复制代码', needsUserActivation: !onMac, run: () => copyText(code) }, { text: '' });
          if (!embed && restyle.length) items.push(...restyle, { text: '' });
          if (canUntick) items.push({ text: '取消勾选此清单的所有任务', run: () => {
            if (editor.state.doc !== before || editor.compositionStarted) return;
            const change = untickChecklist(editor.state, position);
            if (change) editor.dispatch(change);
          } }, { text: '' });
          if (target) {
            const wiki = wikiLinkTarget(target);
            items.push({ text: wiki ? '打开笔记' : standalone?.form.kind === 'file' ? '打开附件' : '打开链接', run: () => openLink(target) });
            if (alternatives.length) items.push({ text: '打开方式', items: alternatives });
            if (!embed || standalone?.form.kind === 'web') items.push({ text: '复制链接', needsUserActivation: !onMac, run: () => copyText(wiki ? `[[${wiki}]]` : target) });
            if (clipboard.length) items.push({ text: '' });
          }
          if (embed && !readOnly && standalone) items.push(...restyle, { text: '' }, { text: '从笔记中移除', destructive: true, run: () => rewrite() });
          return [...items, ...clipboard];
        };
        const items = target && !embed && onMac && external.test(target)
          ? invoke<[string, string, boolean][]>('link_applications', { target }).then(apps => menuItems(apps.map(([application, name, defaultApp]) => ({
            text: defaultApp ? `${name}（默认）` : name,
            run: () => invoke<void>('open_external_link', { target, application }),
          }))), () => menuItems())
          : menuItems();
        void popupMenu(items, event.clientX, event.clientY, message => options.onError?.(message), () => editor.focus(), canUntick ? '任务清单操作' : code ? '代码块操作' : target ? '链接操作' : '编辑操作');
        return true;
      },
    }),
  ];
  const state = (doc = '') => EditorState.create({ doc, extensions: extensions() });
  const view = new EditorView({ parent, state: state() });
  function missingWikiLink(link: HTMLAnchorElement) {
    const target = wikiLinkTarget(link.getAttribute('href') ?? '');
    return target !== null && noteLinkExists?.(target) === false;
  }
  function refreshTableWikiLinks() {
    if (!noteLinkExists) return;
    for (const link of view.contentDOM.querySelectorAll<HTMLAnchorElement>('.cm-markra-table a.cm-markra-wiki-link')) {
      link.toggleAttribute('data-missing-note', missingWikiLink(link));
    }
  }
  function scrollToLine(line: number) {
    const from = view.state.doc.line(Math.min(Math.max(1, line), view.state.doc.lines)).from;
    revealHeadingSection(view, from);
    view.dispatch({ effects: EditorView.scrollIntoView(from, { y: 'start' }) });
  }
  function format(action: Format) {
    if (readOnly || view.compositionStarted) return;
    if (formatActiveTableSelection(view, action)) return;
    const command = commands[action as keyof typeof commands];
    if (!command || !runMarkraCommand(view, command)) view.dispatch(formatTransaction(view.state, action));
    view.focus();
  }
  return {
    view,
    element: view.scrollDOM,
    input: view.contentDOM,
    get composing() { return view.compositionStarted || Boolean(pending); },
    activate(source: string, id: string, attachmentNoteId?: string, attachmentBase?: string, preview = Boolean(options.readOnly)) {
      closeTablePanel?.();
      const resetState = documentId !== id || readOnly !== preview;
      documentId = id; noteId = attachmentNoteId; assetBase = attachmentBase;
      readOnly = preview;
      parent.dataset.readOnly = String(readOnly);
      active = false;
      if (resetState) {
        pending = undefined; captured = undefined;
        view.setState(state(source));
        view.scrollDOM.scrollTop = 0;
      } else {
        const changes = replaceSource(view.state, source)?.changes;
        const effects = [refreshLinkCards.of(null), wikiLinksChanged.of(null), ...(changes ? [reloadHeadingFolds.of(null)] : [])];
        view.dispatch({ changes, effects, annotations: Transaction.addToHistory.of(false) });
        refreshTableWikiLinks();
      }
      view.requestMeasure();
      active = true;
      options.relations?.refresh(view);
    },
    deactivate() { checklist.cancel(); closeTablePanel?.(); closeNoteDatePicker(view); view.plugin(editorHistoryMenuUpdates)?.dismissNotice(); flush(); active = false; },
    refreshNoteLinks() {
      if (!active) return;
      options.relations?.refresh(view);
      if (!noteLinkExists) return;
      const links = view.contentDOM.querySelectorAll<HTMLAnchorElement>('a.cm-markra-wiki-link');
      if (![...links].some(link => link.hasAttribute('data-missing-note') !== missingWikiLink(link))) return;
      view.dispatch({ effects: wikiLinksChanged.of(null) });
      refreshTableWikiLinks();
    },
    flush,
    captureTableMenu(anchor: HTMLElement) { return captureActiveTableMenu(view, anchor); },
    captureTableFormat() { return captureTableFormat(view); },
    paragraphFormat() { return paragraphFormat(view.state); },
    formatPanelState() { return formatPanelState(view.state); },
    formatTargetBounds() { return view.coordsAtPos(view.state.selection.main.head); },
    applyFormatPanel(action: PanelFormat) {
      if (readOnly || view.compositionStarted) return;
      if (['bold', 'italic', 'underline', 'strike', 'highlight', 'callout'].includes(action)) format(action as Format);
      else view.dispatch(formatPanelTransaction(view.state, action));
    },
    pictureSelection() { return pictureSelection(view); },
    get canUntickChecklist() { return !readOnly && Boolean(untickChecklist(view.state)); },
    untickChecklist() {
      if (readOnly || view.compositionStarted) return;
      const change = untickChecklist(view.state);
      if (change) view.dispatch(change);
      view.focus();
    },
    focus() { view.focus(); },
    captureSelection() { captured = view.state.selection; },
    captureTableInsertion() { return captureTableInsertion(view); },
    insert(markdown: string | ((selected: string) => string)) {
      if (readOnly || view.compositionStarted) return;
      const selection = captured ?? view.state.selection;
      captured = undefined;
      view.dispatch(view.state.changeByRange(range => {
        const insert = typeof markdown === 'string' ? markdown : markdown(view.state.sliceDoc(range.from, range.to));
        return {
          changes: { from: range.from, to: range.to, insert },
          range: EditorSelection.cursor(range.from + insert.length),
        };
      }), { scrollIntoView: true, userEvent: 'input.paste' });
      captured = view.state.selection;
      view.focus();
    },
    format,
    scrollToLine,
    find(direction?: 'next' | 'previous') { return findInEditor(view, direction); },
    foldHeadings(expanded: boolean) {
      const id = documentId;
      return foldAllHeadings(view, expanded, () => active && id === documentId);
    },
    requestMeasure() { view.requestMeasure(); },
    /** 正文被外部来源从 `before` 改成了现在的内容：标出改动的行。 */
    markChanges(before: string, who?: string) { return active ? markChanges(view, before, who) : 0; },
  };
}

export type RichEditor = ReturnType<typeof createRichEditor>;
