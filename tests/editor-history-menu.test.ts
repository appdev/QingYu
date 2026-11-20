// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { history } from '@codemirror/commands';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { connectEditorHistoryMenu, refreshEditorHistory } from '../src/editor-history-menu';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ label: 'main' }) }));

it('updates and routes the native menus without waiting for a WebKit repaint', async () => {
  const platform = Object.getOwnPropertyDescriptor(navigator, 'platform');
  Object.defineProperty(navigator, 'platform', { configurable: true, value: 'MacIntel' });
  vi.stubGlobal('__TAURI_INTERNALS__', {});
  vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  Object.defineProperty(document, 'queryCommandEnabled', { configurable: true, value: () => false });
  const source = '# 标题\n\n正文\n\n- [ ] 清单\n- [x] 已完成';
  const view = new EditorView({ parent: document.body, state: EditorState.create({ doc: source,
    extensions: [markdown({ base: markdownLanguage }), history()], selection: { anchor: source.indexOf('清单') },
  }) });
  try {
    const apply = vi.fn();
    await connectEditorHistoryMenu({ current: () => view, apply });
    expect(invoke).toHaveBeenCalledWith('editor_menu_state', {
      undo: false, redo: false, sections: false, tickLabel: '标为已完成', format: { editable: false, ticked: null, untick: false, moves: [false, false] },
    });
    const emit = (event: string, payload: string) => vi.mocked(listen).mock.calls.find(call => call[0] === event)![1]({ event, id: 1, payload });
    emit('note-format', 'title');
    expect(apply).not.toHaveBeenCalled();

    view.focus();
    refreshEditorHistory();
    await Promise.resolve();
    expect(invoke).toHaveBeenLastCalledWith('editor_menu_state', expect.objectContaining({
      format: expect.objectContaining({ editable: true, ticked: false, untick: true }),
    }));
    emit('note-format', 'title');
    expect(apply).toHaveBeenCalledWith('title');
    emit('note-format', 'tick');
    expect(view.state.doc.toString()).toBe(source.replace('[ ]', '[x]'));
    document.documentElement.lang = 'en';
    window.dispatchEvent(new Event('ui-language-change'));
    await Promise.resolve();
    expect(invoke).toHaveBeenLastCalledWith('editor_menu_state', expect.objectContaining({ tickLabel: 'Mark Incomplete' }));
    document.documentElement.lang = '';
    emit('note-format', 'untick');
    expect(view.state.doc.toString()).toBe(source.replace('[x]', '[ ]'));
    emit('editor-history-menu', 'undo');
    expect(view.state.doc.toString()).toBe(source.replace('[ ]', '[x]'));
    emit('editor-history-menu', 'redo');
    expect(view.state.doc.toString()).toBe(source.replace('[x]', '[ ]'));

    const edge = document.createElement('textarea');
    edge.className = 'markra-table-edge-input';
    view.contentDOM.append(edge);
    edge.focus();
    emit('note-format', 'title');
    expect(apply).toHaveBeenCalledTimes(1);
    edge.remove();
  } finally {
    view.destroy();
    await Promise.resolve();
    if (platform) Object.defineProperty(navigator, 'platform', platform);
    else Reflect.deleteProperty(navigator, 'platform');
    Reflect.deleteProperty(document, 'queryCommandEnabled');
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
