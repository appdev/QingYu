// Match MiaoYanCore.Emphasis.markCJK in both rendered and editable text.
export function emphasisLanguage(text: string): string {
  const syllable = text.match(/[\u3040-\u30ff\u31f0-\u31ff\u1100-\u11ff\u3130-\u318f\uac00-\ud7af]/u)?.[0];
  return syllable ? (/[\u3040-\u30ff\u31f0-\u31ff]/u.test(syllable) ? 'ja' : 'ko')
    : /[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\u{20000}-\u{2ffff}]/u.test(text) ? 'zh' : '';
}
