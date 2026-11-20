import { expect, it, vi } from 'vitest';
import { localizeStatus, localizeSyncError, markupLabel, workflowText, type WorkflowMessage } from '../src/ui-language';
import { supportedLanguages } from '../src/vendor/markra/shared/i18n';
import { localizedWorkflowCopy, traditionalWorkflowCopy, workflowCopy, workflowLocales } from '../src/workflow-copy';

it('provides workflow copy in every supported language and preserves interpolation tokens', () => {
  const tokens = (text: string) => (text.match(/\{\w+\}/g) ?? []).sort();
  for (const [key, english] of Object.entries(workflowCopy)) {
    const message = key as WorkflowMessage;
    if (typeof english === 'string') {
      const localized = localizedWorkflowCopy[message];
      expect(localized, key).toHaveLength(workflowLocales.length);
      expect(traditionalWorkflowCopy[message]?.trim(), key).toBeTruthy();
      for (const [index, locale] of workflowLocales.entries()) {
        expect(localized?.[index]?.trim(), `${locale} ${key}`).toBeTruthy();
      }
      if (key !== '轻羽' && key !== '轻羽 QingYu') {
        expect(localized?.every(translation => translation.trim() === english.trim()), key).toBe(false);
      }
    }
    for (const { code } of supportedLanguages) {
      const translation = workflowText(message, code);
      expect(translation.trim(), `${code} ${key}`).not.toBe('');
      expect(tokens(translation), `${code} ${key}`).toEqual(tokens(key));
    }
  }
});

it('uses translated account and sync labels where shared dictionaries contain English copy', () => {
  expect(workflowText('密码', 'fr')).toBe('Mot de passe');
  expect(workflowText('立即同步', 'fr')).toBe('Synchroniser maintenant');
  expect(workflowText('尚未同步', 'ja')).toBe('未同期');
});

it('localizes app-owned sync message headers and preserves paths, HTTP diagnostics and redaction', () => {
  vi.stubGlobal('document', { documentElement: { lang: 'fr' } });
  try {
    const path = ' notes/中文 & $&.md';
    expect(localizeStatus(`无法扫描同步文件：${path}`)).toContain(path);
    const body = '\n<Error>AccessDenied [已隐藏]</Error>';
    expect(localizeStatus('写入测试失败（HTTP 403）' + body)).toBe(
      workflowText('写入测试', 'fr') + ' échoué (HTTP 403)' + body);
    expect(localizeSyncError('S3 网络请求失败：ECONNRESET' + body)).toContain(
      '\n<Error>AccessDenied ' + workflowText('[已隐藏]', 'fr') + '</Error>');
    expect(localizeSyncError('ECONNRESET')).toBe('Échec de la synchronisation : ECONNRESET');
    expect(localizeSyncError('Failed to read "miaoyan-key.json" (HTTP 403): <Error>Denied</Error>')).toBe(
      'Impossible de lire « miaoyan-key.json » (HTTP 403)\n<Error>Denied</Error>');
    expect(localizeStatus('写入测试失败（HTTP 403）' + body + '\n清理测试文件失败（HTTP 403）' + body)).toBe(
      workflowText('写入测试', 'fr') + ' échoué (HTTP 403)' + body + '\n'
      + workflowText('清理测试文件', 'fr') + ' échoué (HTTP 403)' + body);
    expect(localizeStatus('同步密码设置无效')).toBe(workflowText('同步密码设置无效', 'fr'));
    expect(markupLabel('运行中 · {url}', { url: 'http://example/?q=中文&x=<script>' })).toContain(
      'http://example/?q=中文&amp;x=&lt;script&gt;');
  } finally {
    vi.unstubAllGlobals();
  }
});
