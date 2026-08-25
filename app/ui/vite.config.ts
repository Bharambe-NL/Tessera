import { defineConfig } from 'vite';

// The Tauri shell loads the dev server in development and the built assets in
// release. The port is fixed so tauri.conf.json can name it.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5183,
    strictPort: true,
  },
  build: {
    // Matches the webview floor the app ships against.
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: true,
  },
});
