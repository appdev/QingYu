import { defineConfig } from 'vite';

export default defineConfig({
  clearScreen: false,
  build: { target: 'es2022', rollupOptions: { input: ['index.html', 'settings.html', 'preview.html'] } },
  server: { watch: { ignored: ['**/src-tauri/**'] } },
});
