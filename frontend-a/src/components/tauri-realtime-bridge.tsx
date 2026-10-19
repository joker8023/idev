// 实时桥接：Rust 写命令成功后 emit `multica://entity-changed`（{kind, id}），
// 这里监听并失效 React Query 缓存，替代原云端 WebSocket 推送。
// 浏览器预览下无 Tauri 运行时，不订阅（mock 数据由 mutation 失效逻辑处理）。

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";

const inTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function TauriRealtimeBridge() {
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!inTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<{ kind: string; id: string }>("multica://entity-changed", () => {
      // 本地单用户、数据量小：任何实体变更都全量失效，比按 kind 精确失效
      // 更可靠（避免漏掉跨实体派生查询，如 issue 变更影响看板 + 详情 + 统计）。
      void queryClient.invalidateQueries();
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // 监听失败时不 crash：页面仍可通过 mutation 失效 + 手动刷新工作。
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);
  return null;
}
