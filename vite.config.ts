import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  clearScreen: false,
  resolve: {
    alias: {
      $lib: path.resolve("./src/lib"),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // esbuild's minifier mangles a variable reference inside @xterm/xterm's
    // `requestMode` (DECRQM) handler, leaving it as a free `i` that throws
    // `ReferenceError: Can't find variable: i` the first time it fires.
    // That happens on any `CSI ? <n> $ p` query — notably Claude Code's
    // synchronized-output probe (`CSI ? 2026 $ p`), which Claude emits
    // before its TUI renders. The exception is raised from inside xterm's
    // `_innerWrite`, which halts the parser, so no subsequent PTY output
    // shows up and the pane appears frozen. Dev builds (unminified) and
    // CLIs that don't use this query (e.g. Codex) aren't affected.
    minify: false,
  },
});
