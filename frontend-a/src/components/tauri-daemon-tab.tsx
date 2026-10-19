// Tauri 版 Daemon 设置页签（对标桌面版 DaemonSettingsTab）：
//   - 桌面版读 Electron daemonAPI 的 daemon 状态 / 自动启动；
//   - Tauri 版 daemon 在 Rust 进程内，没有独立 daemon 进程可启停，
//     这里展示 Rust 后端探测到的本机 Agent CLI + Runtime 列表。
//     后续接真实启停时，再补 Rust 命令。

import { useQuery } from "@tanstack/react-query";
import { getApi } from "@multica/core/api";
import { useT } from "@multica/views/i18n";
import { Card, CardContent, CardHeader, CardTitle } from "@multica/ui/components/ui/card";

export function TauriDaemonSettingsTab() {
  const { t } = useT("settings");
  const { data: runtimes } = useQuery({
    queryKey: ["tauri", "runtimes"],
    queryFn: () => getApi().listRuntimes(),
  });

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>{t(($) => $.desktop.daemon.title)}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2 text-sm">
          <p>本地 Daemon：内置于应用（Rust 进程内），无需单独启停。</p>
          {(runtimes ?? []).map((r) => (
            <div key={r.id} className="flex items-center justify-between border-t pt-2">
              <span>{r.name}</span>
              <span className="text-muted-foreground">{r.status}</span>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
