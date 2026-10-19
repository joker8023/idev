// Tauri 版路由错误页（对标桌面版 DesktopRouteErrorPage，去掉 tab store）：
//   - 404：可恢复状态，给“回到 Issues”；
//   - 其他：显示错误信息，给“重新加载”。

import { useLocation, useNavigate, useRouteError } from "react-router-dom";
import { Button } from "@multica/ui/components/ui/button";

export function TauriRouteErrorPage() {
  const error = useRouteError();
  const location = useLocation();
  const navigate = useNavigate();
  const message = error instanceof Error ? error.message : "未知路由错误";

  return (
    <div
      role="alert"
      className="flex h-full min-h-[20rem] flex-col items-center justify-center gap-4 p-8 text-center"
    >
      <div className="space-y-2">
        <h2 className="text-title font-semibold">页面加载失败</h2>
        <p className="max-w-lg truncate font-mono text-caption text-muted-foreground">
          {location.pathname}
        </p>
        <p className="max-w-lg truncate text-caption text-muted-foreground">{message}</p>
      </div>
      <div className="flex gap-2">
        <Button type="button" variant="outline" onClick={() => navigate("/default/issues")}>
          回到 Issues
        </Button>
        <Button type="button" onClick={() => window.location.reload()}>
          重新加载
        </Button>
      </div>
    </div>
  );
}
