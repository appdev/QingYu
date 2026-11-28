import { emit, emitTo, listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { interfaceLanguage, mobile, native, preferences, type Preferences } from './preferences';
import { workflowText, type WorkflowMessage } from './ui-language';
import type { SyncClient, SyncStatus } from './sync-session';
import type { S3Settings } from './sync-store';
import type { PasswordAction, PasswordInfo } from './sync-password';

const nativeMenuLabels: WorkflowMessage[] = [
  '轻羽', '设置…', '文件', '编辑', '显示', '窗口', '帮助', '格式', '关于轻羽', '服务', '隐藏轻羽', '隐藏其他应用',
  '退出轻羽', '关闭窗口', '撤销', '重做', '剪切', '复制', '粘贴', '全选', '切换全屏', '最小化', '缩放窗口',
  '查找', '查找…', '查找下一个', '查找上一个', '收起所有章节', '展开所有章节', '放大正文', '缩小正文', '恢复默认字号',
  '标题', '二级标题', '三级标题', '正文', '粗体', '斜体', '下划线', '删除线', '高亮', '行内等宽', '任务清单',
  '标为已完成', '取消勾选全部任务', '项目符号列表', '短划线列表', '编号列表', '块引用', '上移项目', '下移项目',
  '代码块', '分隔线', '标注', '表格', '日期', '链接', '排版优化', '今日笔记', '跳转到笔记…', '打开文件…', '返回笔记库',
  '更改笔记库位置…', '导入文件…', '导入文件夹…', '重新读取笔记文件夹', '新建或查找笔记…', '打开轻羽',
];

let connecting: Promise<void> | undefined;
export function connectPreferences() {
  return connecting ??= connect().catch(error => { connecting = undefined; throw error; });
}

async function connect() {
  if (!native) return;
  let receiving = false;
  await listen<Preferences>('preferences-changed', ({ payload }) => {
    try { receiving = true; preferences.receive(payload); } catch { showNativeError(); } finally { receiving = false; }
  });
  let language = '', menuBar: boolean | undefined;
  const apply = () => {
    const next = interfaceLanguage();
    const labels = !mobile && (language !== next || menuBar !== preferences.value.menuBar)
      ? Object.fromEntries(nativeMenuLabels.map(text => [text, workflowText(text, next)])) : undefined;
    language = next;
    menuBar = preferences.value.menuBar;
    return invoke('apply_preferences', { appearance: preferences.value.appearance, alwaysOnTop: preferences.value.alwaysOnTop,
      menuBar, noteTextSize: preferences.value.previewFontSize, menuLabels: labels }).catch(error => {
      if (labels && language === next) language = '';
      throw error;
    });
  };
  window.addEventListener('ui-language-change', () => {
    if (preferences.value.language === 'system' && language !== interfaceLanguage()) void apply().catch(showNativeError);
  });
  preferences.subscribe(() => { void apply().catch(showNativeError); if (!receiving) void emit('preferences-changed', preferences.value).catch(showNativeError); });
  await apply();
}
function showNativeError() { window.dispatchEvent(new CustomEvent('settings-error', { detail: workflowText('设置已保存，但系统窗口未能更新，请重新打开应用') })); }

type Request = { id: string; action: 'load' | 'save' | 'flush' | 'sync' | 'test' | 'password'; config?: S3Settings; accountId?: string; passwordAction?: PasswordAction };
type Reply = { id: string; config?: S3Settings; password?: PasswordInfo; error?: string };

export function retrySettingsRead(send: () => void) {
  // Opening settings before the main window has registered its listener can drop the first request.
  const interval = setInterval(send, 300);
  send();
  return () => clearInterval(interval);
}

export async function hostSync(client: SyncClient) {
  if (!native || mobile) return;
  await listen<Request>('settings-sync-request', async ({ payload }) => {
    const { id, action, config, accountId, passwordAction } = payload;
    if (typeof id !== 'string' || !['load', 'save', 'flush', 'sync', 'test', 'password'].includes(action)) return;
    const reply: Reply = { id };
    try {
      if (action === 'load') {
        reply.config = await client.load();
        const unsubscribe = client.subscribe(status => { void emitTo('settings', 'settings-sync-status', status); }); unsubscribe();
      }
      else if (action === 'flush') await client.flush();
      else {
        if (!config || typeof config !== 'object' || (accountId !== undefined && typeof accountId !== 'string')) throw new Error('连接设置无效');
        if (action === 'password') {
          if (!client.password || (passwordAction !== undefined && typeof passwordAction?.password !== 'string')) throw new Error('数据仓库密钥设置无效');
          reply.password = await client.password(config, accountId, passwordAction);
        } else if (action === 'save') await client.save(config); else if (action === 'test') await client.test(config); else await client.run(config, accountId);
      }
    } catch (error) { reply.error = error instanceof Error ? error.message : '同步未完成'; }
    await emitTo('settings', 'settings-sync-reply', reply);
  }, { target: 'main' });
  client.subscribe(status => { void emitTo('settings', 'settings-sync-status', status); });
}
export async function remoteSync(): Promise<SyncClient> {
  const pending = new Map<string, { resolve: (reply: Reply) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const listeners = new Set<(status: SyncStatus) => void>();
  let status: SyncStatus = { busy: false, message: '', error: false };
  await listen<Reply>('settings-sync-reply', ({ payload }) => {
    const request = pending.get(payload.id); if (!request) return;
    clearTimeout(request.timer); pending.delete(payload.id);
    if (payload.error) request.reject(new Error(payload.error)); else request.resolve(payload);
  }, { target: 'settings' });
  await listen<SyncStatus>('settings-sync-status', ({ payload }) => { status = payload; for (const listener of listeners) listener(status); }, { target: 'settings' });
  function request(action: Request['action'], config?: S3Settings, accountId?: string, passwordAction?: PasswordAction): Promise<Reply> {
    let stopRetry: (() => void) | undefined;
    return new Promise<Reply>((resolve, reject) => {
      const id = crypto.randomUUID();
      const timeout = action === 'load' ? 10000 : action === 'test' ? (config?.timeout ?? 45) * 6000 + 10000 : 300000;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('主窗口未响应；请保留当前内容，打开主窗口后重试')); }, timeout);
      pending.set(id, { resolve, reject, timer });
      const send = () => { void emitTo('main', 'settings-sync-request', { id, action, config, accountId, passwordAction }).catch(() => {
        clearTimeout(timer); pending.delete(id); reject(new Error('无法连接主窗口'));
      }); };
      if (action === 'load') stopRetry = retrySettingsRead(send);
      else send();
    }).finally(() => stopRetry?.());
  }
  return {
    load: async () => { const { config } = await request('load'); if (!config) throw new Error('无法读取同步设置'); return config; },
    save: async config => { await request('save', config); },
    flush: async () => { await request('flush'); },
    run: async (config, accountId) => { await request('sync', config, accountId); },
    test: async config => { await request('test', config); },
    password: async (config, accountId, action) => {
      const { password } = await request('password', config, accountId, action);
      if (!password) throw new Error('无法读取数据仓库密钥状态');
      return password;
    },
    subscribe(listener) { listeners.add(listener); listener(status); return () => { listeners.delete(listener); }; },
  };
}
