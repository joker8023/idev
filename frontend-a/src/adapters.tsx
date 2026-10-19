// Route A 平台适配层：把原装前端的平台抽象接到 Tauri / 浏览器。
// 原装前端的设计就是平台无关的（AGENTS.md：views 不得 import next/* /
// react-router-dom，平台差异由 adapter 注入）。桌面版（Electron）已经有一套
// adapter（apps/desktop/.../platform/），这里是 Tauri 版的对应实现。

import { useMemo } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { NavigationProvider, type NavigationAdapter } from "@multica/views/navigation";
import { defaultStorage } from "@multica/core/platform";
import type { StorageAdapter } from "@multica/core/types/storage";
import type { LocaleAdapter, SupportedLocale } from "@multica/core/i18n";

export const tauriStorage: StorageAdapter = defaultStorage;

const LOCALE_KEY = "multica-locale";

export const tauriLocaleAdapter: LocaleAdapter = {
  getUserChoice() {
    try {
      return window.localStorage.getItem(LOCALE_KEY);
    } catch {
      return null;
    }
  },
  getSystemPreferences() {
    return typeof navigator !== "undefined" && navigator.language
      ? [navigator.language]
      : [];
  },
  persist(locale: SupportedLocale) {
    try {
      window.localStorage.setItem(LOCALE_KEY, locale);
    } catch {
      // best-effort
    }
  },
};

/** NavigationAdapter ← react-router（MemoryRouter）。与桌面版 adapter 同契约，
//  去掉了桌面版的 tab/overlay 逻辑（Tauri 第一版单窗口；多 tab 后续再接）。 */
export function TauriNavigationProvider({ children }: { children: React.ReactNode }) {
  const navigate = useNavigate();
  const location = useLocation();
  const adapter = useMemo<NavigationAdapter>(
    () => ({
      push: (path: string) => navigate(path),
      replace: (path: string) => navigate(path, { replace: true }),
      back: () => navigate(-1),
      pathname: location.pathname,
      searchParams: new URLSearchParams(location.search),
      hash: location.hash,
      getShareableUrl: (path: string) => path,
      canGoBack: () => window.history.length > 1,
      forward: () => navigate(1),
    }),
    [navigate, location],
  );
  return <NavigationProvider value={adapter}>{children}</NavigationProvider>;
}
