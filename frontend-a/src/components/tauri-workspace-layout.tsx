// Tauri 版 WorkspaceRouteLayout（对标桌面版）：
//   - 从 URL 读 :workspaceSlug，在 React Query 列表里解析出 workspace；
//   - setCurrentWorkspace(slug, wsId) + WorkspaceSlugProvider，供子页面
//     useCurrentWorkspace / useWorkspaceId 读取；
//   - workspace 未解析出来之前不渲染子页面（否则 useWorkspaceId throw）。
// 桌面版还处理 tab store / overlay / 欢迎弹窗，Tauri 单窗口版都不需要。

import { useEffect } from "react";
import { Outlet, useParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { workspaceBySlugOptions } from "@multica/core/workspace";
import { setCurrentWorkspace } from "@multica/core/platform";

export function TauriWorkspaceLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { data: workspace } = useQuery(
    workspaceBySlugOptions(workspaceSlug ?? ""),
  );

  useEffect(() => {
    if (workspace) {
      setCurrentWorkspace(workspace.slug, workspace.id);
    }
  }, [workspace]);

  if (!workspaceSlug) return null;
  if (!workspace) {
    return (
      <div className="flex h-full items-center justify-center text-sm">
        加载工作区…
      </div>
    );
  }

  // WorkspaceSlugProvider 在壳层（RootLayout）统一提供，这里只负责
  // setCurrentWorkspace 同步单例，不再嵌套第二层 Provider。
  return <Outlet />;
}
