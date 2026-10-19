// Route A 壳：原装前端（vendor/）+ Tauri 传输 + 最小平台适配。
//
// 接线 = 桌面版 App.tsx 的 Tauri 等价（CoreProvider 被拆开，因为它内部
// new ApiClient(apiBaseUrl) —— 本地版不走 HTTP，改走 tauri-api 的 Proxy）：
//   QueryClientProvider + I18nProvider + ThemeProvider + RouterProvider
//   + 注册本地 auth store + BootGate（等 workspace 列表）→ 全部原装页面。
//
// 浏览器预览（vite dev/preview）用 mock 数据；Tauri 里走 Rust invoke。

import { useState } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { RouterProvider } from "react-router-dom";
import { I18nProvider } from "@multica/core/i18n/react";
import { RESOURCES } from "@multica/views/locales";
import { createAuthStore, registerAuthStore, useAuthStore } from "@multica/core/auth";
import { createChatStore, registerChatStore } from "@multica/core/chat";
import { getApi } from "@multica/core/api";
import { setCurrentWorkspace } from "@multica/core/platform";
import { workspaceListOptions } from "@multica/core/workspace/queries";
import { ThemeProvider } from "@multica/ui/components/common/theme-provider";
import { installTauriApi, MOCK_USER, MOCK_WORKSPACE } from "./tauri-api";
import { tauriStorage } from "./adapters";
import { createTauriRouter } from "./routes";
import { TauriRealtimeBridge } from "./components/tauri-realtime-bridge";

import { WSProvider } from "@multica/core/realtime";

let booted = false;
let authStoreInstance: ReturnType<typeof createAuthStore> | null = null;
function bootOnce() {
  if (booted) return;
  booted = true;
  installTauriApi();
  // 本地版无云端登录：直接以本地用户完成认证，跳过 AuthInitializer / 邮箱验证码 / OAuth。
  const store = createAuthStore({ api: getApi(), storage: tauriStorage });
  authStoreInstance = store;
  registerAuthStore(store);
  useAuthStore.setState({ user: MOCK_USER, isLoading: false, status: "authenticated" });
  // CoreProvider 还会建 chat store（FloatingChat 用）；本地版同样注册一个，
  // 否则壳一挂 FloatingChat 就 throw “Chat store not initialised”。
  registerChatStore(createChatStore({ storage: tauriStorage }));
  setCurrentWorkspace(MOCK_WORKSPACE.slug, MOCK_WORKSPACE.id);
}

export default function App() {
  bootOnce();
  const [queryClient] = useState(() => new QueryClient());
  const [router] = useState(() => createTauriRouter());
  return (
    <QueryClientProvider client={queryClient}>
      <TauriRealtimeBridge />
      <I18nProvider locale="en" resources={RESOURCES}>
        <ThemeProvider>
          <BootGate>
            {/* 原装 WSProvider：token 模式下无 token 不建连接（只提供 subscribe/
                onReconnect 空实现），所以这里挂它 = 纯上下文占位，Runtimes 等页
                的 useWS 不再 throw。真推送走 TauriRealtimeBridge（Rust 写命令
                emit → 前端失效缓存），见 components/tauri-realtime-bridge.tsx。 */}
            <WSProvider
              wsUrl="ws://127.0.0.1:1/ws"
              authStore={authStoreInstance!}
              storage={tauriStorage}
            >
              <RouterProvider router={router} />
            </WSProvider>
          </BootGate>
        </ThemeProvider>
      </I18nProvider>
    </QueryClientProvider>
  );
}

/**
 * 启动门：useCurrentWorkspace = slug + React Query workspace 列表。
 * 列表还没回来时它返回 null，而 useWorkspaceId 遇到 null 直接 throw，
 * 所以必须等列表成功后再挂 workspace 路由（桌面版 AppContent 的
 * bootstrapping 标志做的是同一件事）。
 */
function BootGate({ children }: { children: React.ReactNode }) {
  const { isSuccess, isError } = useQuery(workspaceListOptions());
  if (isError) {
    return (
      <div className="flex h-screen items-center justify-center text-sm text-destructive">
        工作区列表加载失败（tauri-api.listWorkspaces 未迁移或出错）。
      </div>
    );
  }
  if (!isSuccess) {
    return <div className="flex h-screen items-center justify-center text-sm">加载中…</div>;
  }
  return <>{children}</>;
}
