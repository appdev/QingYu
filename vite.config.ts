import { defineConfig } from 'vite';

export default defineConfig({
  clearScreen: false,
  build: { target: 'es2022', rollupOptions: { input: ['index.html', 'settings.html', 'preview.html', 'login.html'] } },
  server: { watch: { ignored: ['**/src-tauri/**'] } },
});
