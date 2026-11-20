// Suites that assert Chinese copy use a fixed host locale.
Object.defineProperties(navigator, {
  language: { configurable: true, value: 'zh-CN' },
  languages: { configurable: true, value: ['zh-CN'] },
});
