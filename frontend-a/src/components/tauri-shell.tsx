// Tauri 壳 = 桌面版 DesktopShell 去掉 Electron 多 tab 部分：
//   SidebarProvider + 原装 AppSidebar + 主区 Outlet + 原装 ModalRegistry /
//   SearchCommand / FloatingChat / GlobalShortcuts / NavigationProgress。
// TabBar / TabContent / WindowOverlay / WindowToolbar 是 Electron 多窗口逻辑，
// Tauri 第一版单窗口不要。

import { Outlet } from "react-router-dom";
import { SidebarProvider } from "@multica/ui/components/ui/sidebar";
import { ModalRegistry } from "@multica/views/modals/registry";
import {
  AppSidebar,
  GlobalShortcuts,
  NavigationProgress,
} from "@multica/views/layout";
import { SearchCommand } from "@multica/views/search";
import { FloatingChat } from "@multica/views/chat/floating-chat";
import { useWorkspaceSlug } from "@multica/core/paths";

export function TauriShell() {
  // 壳级组件（AppSidebar / ModalRegistry / SearchCommand / FloatingChat /
  // GlobalShortcuts）内部调 useRequiredWorkspaceSlug / useWorkspaceId，
  // slug 为 null 时会 throw —— 桌面版同样门控（DesktopShell 的 slug && …）。
  const slug = useWorkspaceSlug();
  return (
    <SidebarProvider>
      <div className="flex h-screen w-full overflow-hidden">
        {slug && <AppSidebar />}
        <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-auto">
            <Outlet />
          </div>
        </main>
      </div>
      {slug && <ModalRegistry />}
      {slug && <SearchCommand />}
      {slug && <FloatingChat />}
      {slug && <GlobalShortcuts />}
      <NavigationProgress />
    </SidebarProvider>
  );
}
