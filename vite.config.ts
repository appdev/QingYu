import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import { palettePlugin } from './tools/palette';

export default defineConfig({
  clearScreen: false,
  resolve: { alias: {
    '@markra/editor/codemirror': fileURLToPath(new URL('./src/vendor/markra/editor/codemirror/index.ts', import.meta.url)),
    '@markra/shared': fileURLToPath(new URL('./src/vendor/markra/shared/index.ts', import.meta.url)),
    '@markra/markdown': fileURLToPath(new URL('./src/vendor/markra/markdown/index.ts', import.meta.url)),
  } },
  optimizeDeps: { exclude: ['@codemirror/view'] },
  plugins: [{
    name: 'codemirror-wkwebview-focus',
    transform(code, id) {
      if (!id.endsWith('/@codemirror/view/dist/index.js')) return;
      // Upstream Markra's WKWebView fix. The wrapper omits Safari's Version token.
      return code.replace('browser.safari && browser.safari_version >= 26', 'browser.safari && (browser.safari_version >= 26 || browser.safari_version === 0)');
    },
  }],
  css: { postcss: { plugins: [palettePlugin] } },
  build: { target: 'es2022', rollupOptions: { input: ['index.html', 'settings.html', 'login.html', 'menubar.html'] } },
  server: { watch: { ignored: ['**/src-tauri/**'] } },
});
