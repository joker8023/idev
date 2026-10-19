import { Server } from "lucide-react";
import { SettingsPage } from "@multica/views/settings";
import { useT } from "@multica/views/i18n";
import { TauriDaemonSettingsTab } from "./tauri-daemon-tab";

// 对标桌面版 DesktopSettingsRoute：原装 SettingsPage + 本机 Daemon 页签。
// 桌面版还有 Updates 页签（Electron 自动更新）；Tauri 更新器后续再接，这里不放。
export function TauriSettingsRoute() {
  const { t } = useT("settings");
  return (
    <SettingsPage
      extraDeviceTabs={[
        {
          value: "daemon",
          label: t(($) => $.desktop.daemon.title),
          icon: Server,
          content: <TauriDaemonSettingsTab />,
        },
      ]}
    />
  );
}
