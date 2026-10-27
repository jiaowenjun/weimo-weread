import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// 开发期把 /api 与 /healthz 代理到本地服务器，前端无需处理 CORS
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    proxy: {
      '/api': { target: 'http://127.0.0.1:8081', changeOrigin: true },
      '/healthz': { target: 'http://127.0.0.1:8081', changeOrigin: true },
    },
  },
});
