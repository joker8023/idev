import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1421, strictPort: true },
  build: {
    chunkSizeWarningLimit: 1024,
    rollupOptions: {
      output: {
        // 主包 6.3MB → 拆包：mermaid / lowlight / tiptap / vendor 单独 chunk，
        // 首屏只加载业务 chunk。manualChunks 按包名匹配（node_modules 路径）。
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("mermaid")) return "mermaid";
          if (id.includes("lowlight") || id.includes("highlight.js")) return "lowlight";
          if (id.includes("@tiptap")) return "tiptap";
          if (id.includes("react-router") || id.includes("@remix-run")) return "router";
          if (
            id.includes("react") ||
            id.includes("react-dom") ||
            id.includes("scheduler")
          ) {
            return "react-vendor";
          }
          if (id.includes("@tanstack")) return "tanstack";
          if (id.includes("framer-motion") || id.includes("motion")) return "motion";
          if (id.includes("recharts") || id.includes("d3-")) return "charts";
          return "vendor";
        },
      },
    },
  },
});
