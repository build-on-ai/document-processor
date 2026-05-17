import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  css: {
    postcss: null
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Tauri 2 ships modern WebKit (Chrome ~115+ on Linux WebKit2GTK 4.1,
    // WebView2 Edge ~115+ on Windows). Svelte 5 emits private-class /
    // class-field syntax that the older es2021/chrome100/safari13
    // targets cannot lower, so production builds failed against them.
    target: ['es2022', 'chrome115', 'safari16'],
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
