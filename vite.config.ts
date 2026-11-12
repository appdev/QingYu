import { defineConfig } from 'vite';
import { palettePlugin } from './tools/palette';

export default defineConfig({
  clearScreen: false,
  css: { postcss: { plugins: [palettePlugin] } },
  build: { target: 'es2022', rollupOptions: { input: ['index.html', 'settings.html', 'preview.html', 'login.html'] } },
  server: { watch: { ignored: ['**/src-tauri/**'] } },
});
