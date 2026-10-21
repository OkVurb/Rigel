import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri loads the built files from ./dist; the dev server is only used with `tauri dev`.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true },
});
