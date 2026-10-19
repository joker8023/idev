// Tauri 路由表 = 桌面版 routes.tsx 的 Tauri 等价（workspace 作用域 `/:workspaceSlug/...`）。
// 差异：
//   - 单窗口，无 tab：去掉 TabBar/TabContent/WindowOverlay，根布局 = TauriShell；
//   - 无登录页：本地用户已在 App 启动时 seed；
//   - DesktopSettingsRoute → TauriSettingsRoute（Daemon 页签，暂无 Updates 页签）；
//   - DesktopAgentsPage/DesktopRuntimesPage → Tauri 版（常量本机身份）。

import { useEffect, useSyncExternalStore } from "react";
import { Navigate, createMemoryRouter, useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { WorkspaceSlugProvider } from "@multica/core/paths";
import { getCurrentSlug, subscribeToCurrentSlug } from "@multica/core/platform";
import { workspaceListOptions } from "@multica/core/workspace";
import { IssuesPage } from "@multica/views/issues/components";
import { ProjectsPage } from "@multica/views/projects/components";
import { DashboardPage } from "@multica/views/dashboard";
import { AutopilotsPage } from "@multica/views/autopilots/components";
import { MyIssuesPage } from "@multica/views/my-issues";
import { SkillsPage } from "@multica/views/skills";
import {
  AiCreateAgentPage,
  ChooseCreateMethodPage,
  ManualCreateAgentPage,
} from "@multica/views/agents";
import { SquadsPage, SquadDetailPage as SquadDetailPageView } from "@multica/views/squads/components";
import { InboxPage } from "@multica/views/inbox";
import { ChatPage } from "@multica/views/chat";
import { SettingsPage } from "@multica/views/settings";
import { TauriNavigationProvider } from "./adapters";
import { TauriShell } from "./components/tauri-shell";
import { TauriWorkspaceLayout } from "./components/tauri-workspace-layout";
import { TauriRouteErrorPage } from "./components/route-error-page";
import { TauriAgentsPage } from "./components/tauri-agents-page";
import { TauriRuntimesPage } from "./components/tauri-runtimes-page";
import { TauriSettingsRoute } from "./components/tauri-settings-route";
import { IssueDetailPage } from "./pages/issue-detail-page";
import { ProjectDetailPage } from "./pages/project-detail-page";
import { AutopilotDetailPage } from "./pages/autopilot-detail-page";
import { SkillDetailPage } from "./pages/skill-detail-page";
import { AgentDetailPage } from "./pages/agent-detail-page";
import { AiBuilderSessionPage } from "./pages/ai-builder-session-page";
import { MemberDetailPage } from "./pages/member-detail-page";
import {
  RuntimeDetailPage,
  RuntimeSettingsPage,
} from "./pages/runtime-detail-page";
import { AttachmentPreviewRoute } from "./pages/attachment-preview-page";

declare global {
  interface Window {
    // 调试钩子：Playwright 爬路由时用（MemoryRouter 不读地址栏）。
    __tauriNavigate?: (path: string) => void;
  }
}

function RootLayout() {
  const navigate = useNavigate();
  useEffect(() => {
    window.__tauriNavigate = (path: string) => navigate(path);
  }, [navigate]);
  // 桌面版同款：slug 从平台单例（setCurrentWorkspace 写入）+ 列表缓存校验。
  // WorkspaceSlugProvider 包住整个壳（slug 可为 null），壳内组件按 slug 门控，
  // 否则 AppSidebar 里的 useRequiredWorkspaceSlug 会在壳层直接 throw。
  const currentSlug = useSyncExternalStore(
    subscribeToCurrentSlug,
    getCurrentSlug,
    () => null,
  );
  const { data: workspaces = [] } = useQuery(workspaceListOptions());
  const slug =
    currentSlug && workspaces.some((w) => w.slug === currentSlug)
      ? currentSlug
      : null;
  return (
    <WorkspaceSlugProvider slug={slug}>
      <TauriNavigationProvider>
        <TauriShell />
      </TauriNavigationProvider>
    </WorkspaceSlugProvider>
  );
}

export function createTauriRouter() {
  return createMemoryRouter(
    [
      {
        path: "/",
        element: <RootLayout />,
        errorElement: <TauriRouteErrorPage />,
        children: [
          { index: true, element: <Navigate to="/default/issues" replace /> },
          {
            path: ":workspaceSlug",
            element: <TauriWorkspaceLayout />,
            children: [
              { path: "issues", element: <IssuesPage /> },
              { path: "issues/:id", element: <IssueDetailPage /> },
              { path: "projects", element: <ProjectsPage /> },
              { path: "projects/:id", element: <ProjectDetailPage /> },
              { path: "autopilots", element: <AutopilotsPage /> },
              { path: "autopilots/:id", element: <AutopilotDetailPage /> },
              { path: "my-issues", element: <MyIssuesPage /> },
              { path: "runtimes", element: <TauriRuntimesPage /> },
              { path: "runtimes/:id", element: <RuntimeDetailPage /> },
              { path: "runtimes/:id/runtime/:runtimeId", element: <RuntimeSettingsPage /> },
              { path: "skills", element: <SkillsPage /> },
              { path: "skills/:id", element: <SkillDetailPage /> },
              { path: "agents", element: <TauriAgentsPage /> },
              { path: "agents/new", element: <ChooseCreateMethodPage /> },
              { path: "agents/new/manual", element: <ManualCreateAgentPage /> },
              { path: "agents/new/ai", element: <AiCreateAgentPage /> },
              { path: "agents/new/ai/:sessionId", element: <AiBuilderSessionPage /> },
              { path: "agents/:id", element: <AgentDetailPage /> },
              { path: "members/:id", element: <MemberDetailPage /> },
              { path: "squads", element: <SquadsPage /> },
              { path: "squads/:id", element: <SquadDetailPageView /> },
              { path: "inbox", element: <InboxPage /> },
              { path: "chat", element: <ChatPage /> },
              { path: "attachments/:id/preview", element: <AttachmentPreviewRoute /> },
              { path: "usage", element: <DashboardPage /> },
              { path: "settings", element: <TauriSettingsRoute /> },
            ],
          },
          { path: "*", element: <TauriRouteErrorPage /> },
        ],
      },
    ],
    { initialEntries: ["/default/issues"] },
  );
}

// 保留：SettingsPage 裸导出（测试/后续直接用时不需要 Tauri 包装）。
export { SettingsPage };
