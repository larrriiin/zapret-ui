import { defineConfig } from 'vite';

// Vite configuration for the Tauri frontend.
// - `src/` holds the entry `index.html`, modules, HTML fragments and assets.
// - Production build is emitted to `dist/` at the repo root; Tauri reads it via
//   `build.frontendDist` in `src-tauri/tauri.conf.json`.
// - Dev server listens on the Tauri-standard port 1420 and is attached to
//   `tauri dev` through `build.devUrl`.
export default defineConfig({
  root: 'src',
  publicDir: false,
  base: './',
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'esnext',
  },
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  clearScreen: false,
});
