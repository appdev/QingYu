import { markupLabel } from './ui-language';
import {
  Bold, Highlighter, IndentDecrease, IndentIncrease, Italic, List, ListMinus, ListOrdered,
  MessageSquareText, Minus, Strikethrough, TextQuote, Underline, X,
} from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import type { Format, PanelFormat, formatPanelState } from './format';
import { showEditorInputPanel } from './editor-viewport';

export const inlineFormatIcons: Partial<Record<Format, typeof Bold>> = {
  bold: Bold, italic: Italic, underline: Underline, strike: Strikethrough, highlight: Highlighter,
};
const styles = [
  ['title', '标题'], ['heading', '二级标题'], ['subheading', '三级标题'], ['body', '正文'], ['codeblock', '等宽'],
] as const;
const rows = [
  [[['bold', '粗体', inlineFormatIcons.bold], ['italic', '斜体', inlineFormatIcons.italic],
    ['underline', '下划线', inlineFormatIcons.underline], ['strike', '删除线', inlineFormatIcons.strike]],
    [['highlight', '高亮', inlineFormatIcons.highlight]]],
  [[['bullet', '项目符号列表', List], ['list', '短划线列表', ListMinus], ['ordered', '编号列表', ListOrdered]],
    [['outdent', '减少缩进', IndentDecrease], ['indent', '增加缩进', IndentIncrease]]],
  [[['quote', '块引用', TextQuote], ['divider', '分隔线', Minus]], [['callout', '标注', MessageSquareText]]],
] as const;
export function openEditorFormatPanel(
  trigger: HTMLButtonElement,
  read: () => ReturnType<typeof formatPanelState> & { inTable?: boolean; canFormatCell?: boolean },
  apply: (action: PanelFormat) => void,
  onClose: (restore: boolean) => void,
  input?: HTMLElement,
) {
  const doc = trigger.ownerDocument;
  const dialog = doc.createElement('dialog');
  dialog.className = 'format-panel';
  dialog.id = 'editor-format-panel';
  dialog.setAttribute('aria-labelledby', 'editor-format-panel-title');
  dialog.innerHTML = `<header><h2 id="editor-format-panel-title">${markupLabel('格式')}</h2><button type="button" aria-label="${markupLabel('关闭格式面板')}"></button></header><div class="format-panel-styles" role="group" aria-label="${markupLabel('段落样式')}"></div>`;
  const closeButton = dialog.querySelector<HTMLButtonElement>('header button')!;
  closeButton.append(createLucideIcon(doc, X, 'format-panel-icon'));
  const controls = new Map<PanelFormat, HTMLButtonElement>();
  function addButton(container: Element, action: PanelFormat, label: string, icon?: Parameters<typeof createLucideIcon>[1]) {
    const button = doc.createElement('button');
    button.type = 'button';
    button.dataset.format = action;
    button.ariaLabel = label;
    button.append(icon ? createLucideIcon(doc, icon, 'format-panel-icon') : label);
    button.onclick = () => { apply(action); refresh(); };
    container.append(button);
    controls.set(action, button);
  }
  function refresh() {
    const state = read();
    for (const [action, button] of controls) {
      if (action !== 'indent' && action !== 'outdent' && action !== 'divider') button.ariaPressed = String(state.active.has(action));
      const inline = Object.hasOwn(inlineFormatIcons, action);
      button.disabled = state.inTable ? !inline || state.canFormatCell === false
        : action === 'indent' ? !state.canIndent
        : action === 'outdent' ? !state.canOutdent
        : action === 'codeblock' ? state.isTitle
        : state.inCode && (inline || action === 'callout' || action === 'divider');
    }
  }
  const styleRow = dialog.querySelector('.format-panel-styles')!;
  for (const [action, label] of styles) addButton(styleRow, action, label);
  for (const row of rows) {
    const line = doc.createElement('div');
    line.className = 'format-panel-row';
    for (const group of row) {
      const pill = doc.createElement('div');
      pill.className = 'format-panel-pill';
      pill.style.flexGrow = String(group.length);
      for (const [action, label, icon] of group) addButton(pill, action, label, icon);
      line.append(pill);
    }
    dialog.append(line);
  }
  function close(restore = false) {
    if (!dialog.isConnected) return;
    inputPanel.close();
    trigger.ariaExpanded = 'false';
    onClose(restore);
  }
  closeButton.onclick = () => close(true);
  refresh();
  const inputPanel = showEditorInputPanel(dialog, input);
  trigger.ariaExpanded = 'true';
  return { dialog, close, refresh, useInput: inputPanel.useInput };
}
