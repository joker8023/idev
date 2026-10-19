# Route A：原装前端搬迁（Kai 已选 A，2026-10-15）

把原仓库 `packages/core + packages/ui + packages/views + packages/plugin-sdk`
**原样**搬进 `vendor/`，在 `frontend-a/`（Vite + React Router + Tauri）里跑起来，
样式 100% 保留。不走 Next.js，不走云端。

## 为什么可行（盘点实测）

| 盘点 | 结果 |
|---|---|
| `packages/core/ui/views` 里 `from "next/*"` | **0 处**。共享包本身就是框架无关的 |
| `react-router` 在共享包里 | 仅 4 个文件（views navigation context/types、core paths/hooks、members 卡片），且都是 adapter 契约，不是路由实现 |
| `next-themes`（ui 的 ThemeProvider） | 名字带 next，实际是框架无关的 React 库，Vite 下可直接用，不用换 |
| 原 `api` 单例 | `packages/core/api/index.ts` 本身就是 Proxy + `setApiInstance()`，调用方只认 `api.<method>`，换传输层不用改调用方 |
| 桌面版（Electron） | 已证明 views 能在 Next.js 之外跑：`apps/desktop/.../App.tsx + routes.tsx + platform/navigation.tsx` 就是模板 |

## 边界：什么原样搬，什么替换

- **原样搬**：`vendor/` 全部 UI、视图、hooks、stores、i18n 词条、设计 tokens（`tokens.css` / `base.css`）、Tailwind 主题。
- **替换 1：传输层**（`frontend-a/src/tauri-api.ts`）：`ApiClient`（5239 行 fetch + cookie + CSRF，约 400 方法）不搬。
  改用 Proxy：已迁移的方法 → Tauri `invoke`（真 Rust 命令）；其余方法 → 浏览器验证模式走 mock（shape 按原 ApiClient/类型/测试 fixture 写），未迁移的方法 → 抛 `[tauri-api] <method> not yet migrated`，不静默造假。
  迁移进度 = handler 表长度。已接的 invoke：`listWorkspaces / listIssues / createIssue / updateIssue / moveIssue / listAgents / listRuntimes`。
  已接的 mock（浏览器验证用，逐路由爬取补齐）：`getMe / getWorkspace / listComments / createComment / listLabels / listMembers / listProjects / listSquads / listIssueViews / listIssueProperties / listIssueStatuses / listIssueWakeupSummaries / getWorkspaceWorkingAgents / getWorkspaceAgentActivity30d / getWorkspaceAgentRunCounts / listIssueTableRows / listIssueTableFacets / getIssueViewPreference / listPins / listProperties / getChildIssueProgress / listPausedWakeups / getAgentTaskSnapshot / getBaseUrl / listMyInvitations / getInboxUnreadSummary / listChatSessions / listPendingChatTasks / hasAnyPendingChatTasks / getIssue / getProject / listAutopilots / getAutopilot / listAutopilotRuns / listSkills / getSkill / listAgentBuilderSessions / listChatDraftRestores / listChatMessages / getPendingChatTask / getAgent / getSquad / listSquadMembers / getSquadMemberStatus / listInbox / getAttachment / getDashboardUsageDaily / getDashboardUsageByAgent / getDashboardAgentRunTime / getDashboardRunTimeDaily / getDashboardFailuresDaily / getDashboardFailuresByAgent / getSearchIndexManifest / getSearchIndexSnapshot / listRuntimeProfiles / listLabelsForResource / saveAgentBuilderDraft / listTasksByIssue / listIssueWakeups / listIssueSystemWakeups / listIssueDuplicates / listQuickActions / getAssigneeFrequency / listIssuePullRequests / listTimeline / listIssueSubscribers / listAttachments / listChildIssues / listLabelsForIssue / listProjectResources / listAgentTasksPage / listLarkInstallations / listSlackInstallations / listDingTalkInstallations / listWecomInstallations / listTelegramInstallations`。
  注意：Issues 页实际不走 `listIssues`，走表模型（`listIssueTableRows / listIssueTableFacets` 等）。
- **替换 2：平台适配**（`frontend-a/src/adapters.tsx`）：`NavigationAdapter`（react-router MemoryRouter）、`StorageAdapter`（localStorage）、`LocaleAdapter`（localStorage）。照抄桌面版契约，去掉 tab/overlay 逻辑（第一版单窗口）。
- **替换 3：启动接线**（`frontend-a/src/App.tsx`）：不直接用 `CoreProvider`，因为它内部 `new ApiClient(apiBaseUrl)`。
  拆开重接：QueryClientProvider + I18nProvider + ThemeProvider + WSProvider（见实时）+ RouterProvider + 本地 auth store（`createAuthStore` + `registerAuthStore`，直接 seed 本地用户，跳过 AuthInitializer）+ 本地 chat store（`createChatStore` + `registerChatStore`，FloatingChat 用）+ `setCurrentWorkspace`。
- **替换 4：壳与路由**（`frontend-a/src/routes.tsx + components/tauri-shell.tsx`）：桌面版 `routes.tsx` 的 Tauri 等价，
  26 条 workspace 作用域路由全部接通（Issues / Projects / Dashboard(usage) / Autopilots / MyIssues / Skills / Agents / Squads / Inbox / Chat / Settings / Members / Runtimes + 各详情页 + agents/new 四级 + attachments 预览）。
  壳 = 桌面版 DesktopShell 去 Electron 多 tab：SidebarProvider + 原装 AppSidebar + Outlet + 原装 ModalRegistry / SearchCommand / FloatingChat / GlobalShortcuts / NavigationProgress。
  桌面版特定页：DesktopAgentsPage/DesktopRuntimesPage → Tauri 版（本机身份常量 `local` / `本机`，见 `local-machine.ts`）；DesktopSettingsRoute → Tauri 版（原装 SettingsPage + 本机 Daemon 页签，暂无 Updates 页签）；DesktopRouteErrorPage → Tauri 版（无 tab store，回到 Issues / 重新加载）。
- **删除**：`posthog-js` 遥测（core/analytics → 本地版应改为 no-op，尚未接线）、邮箱验证码 / Google OAuth / PAT（`sendCode / verifyCode / google` 不进 handler 表）、`AuthInitializer` / `session-renewal` / cookie 会话（本地无会话）。
- **实时**：原装 `WSProvider` 直接挂上；token 模式下本地 storage 无 token，WSClient 不建连接，只提供 subscribe/onReconnect 空实现（Runtimes 等页的 `useWS` 不再 throw）。后续用 Tauri `listen` 事件桥接真推送。

## 已验证（2026-10-15，Playwright headless Chromium 实测）

- `pnpm install`：`vendor/* + frontend-a` workspace 解析通过（catalog 沿用原仓库；`@vitejs/plugin-react` 必须用 v5，v6 与 Vite 6 的 `vite/internal` 导出冲突）。
- `pnpm --filter frontend-a typecheck`：干净。修了 2 处 vendor 补丁（见下）。
- `pnpm --filter frontend-a build`：成功，`dist/assets/index-*.js` 6.3MB（未拆包，含 mermaid/lowlight/tiptap 全量；后续用 dynamic import 拆包）。
- **运行时渲染：已验证**。原装 `IssuesPage` 在 headless Chromium 里真实渲染：
  截图 `docs/route-a-proof.png`：顶部 Issues 标题 + All/Members/Agents 页签 + Filter/Display/Board 按钮，
  5 列看板（Backlog / Todo / In Progress / In Review / Blocked，含原配色与状态图标），
  3 张 mock 卡片落在正确列（拖拽改状态→Backlog，看板能渲染→Todo，Agent 指派→In Progress），
  空列显示 No issues。27 次 `api.*` 调用全部 ok，0 失败，0 页面错误。
- 调试钩子：`window.__tauriApiLog` 记录每次 api 调用（方法 + ok/错误），定位缺失方法全靠它；`window.__tauriNavigate(path)` 供 Playwright 爬路由（MemoryRouter 不读地址栏）。

### 证明过程中修掉的 3 个真坑（已进代码）

1. **Tailwind v4 只扫描 Vite root**：`vendor/` 在 root 之外，utility class 全没生成，页面垮成无样式。
   修：`frontend-a/src/globals.css` 加 `@source "../../vendor/**/*.tsx"` + `@source "../../vendor/**/*.ts"`（路径相对 CSS 文件所在 `src/`，是 `../../vendor`，不是 `../vendor`）。修后 CSS 68KB → 328KB。
2. **workspace slug 必须进 Context**：`useCurrentWorkspace` 从 `WorkspaceSlugProvider` 读 slug，不是 `setCurrentWorkspace` 的模块变量。
   修：`App.tsx` 用 `<WorkspaceSlugProvider slug="default">` 包路由（同桌面版 `WorkspaceRouteLayout`）。
3. **workspace 列表必须先回来**：`useCurrentWorkspace` = slug + React Query 列表；列表还在加载时它返回 null，`useWorkspaceId` 遇 null 直接 throw，整页白屏。
   修：`App.tsx` 加 `BootGate`，等 `workspaceListOptions()` 成功后再挂 workspace 路由（同桌面版 `AppContent` 的 bootstrapping 标志）。

### 全路由验证（2026-10-16，Playwright 逐路由爬取）

- 26 条路由全部渲染，0 页面错误，api 调用 0 失败（四轮爬取，每轮补齐新发现的 FAIL 方法）。
- 截图：`docs/route-a-full-shell-issues.png`（完整壳 + 侧栏 + 看板）、`docs/route-a-full-shell-agents.png`（Agents 列表，Mine 1 / All 2）、`docs/route-a-full-shell-settings.png`。
- Tauri 接线：`src-tauri/tauri.conf.json` 的 `frontendDist` → `../frontend-a/dist`，`devUrl` → `http://localhost:1421`，`beforeBuildCommand / beforeDevCommand` → `pnpm --filter @multica-tauri/frontend-a build/dev`；旧 `frontend/` 骨架已删除（可恢复 trash_id `20261015-172524-91164f5265c2495fb73a08bcd22d16f1`，过期 2026-11-14）。`cargo check` 通过（仅死代码警告）。

### 全路由爬取修掉的问题（已进代码）

4. **壳级 Provider 必须在壳层，不在路由层**：`AppSidebar` 内部调 `useRequiredWorkspaceSlug`，壳在 `WorkspaceSlugProvider` 之外会直接 throw。
   修：`RootLayout` 用桌面版同款（单例 slug + 列表缓存校验）提供 `WorkspaceSlugProvider` 包住整个壳；`TauriWorkspaceLayout` 只负责 `setCurrentWorkspace`，不再嵌套 Provider；壳内组件（AppSidebar / ModalRegistry / SearchCommand / FloatingChat / GlobalShortcuts）按 `slug && …` 门控。
5. **Chat store 也要注册**：`FloatingChat` 调 `useChatStore`，未注册时 throw。
   修：`App.tsx` boot 时 `registerChatStore(createChatStore({ storage }))`（同 auth store）。
6. **Runtimes 页要 WSProvider**：`useWS` 未在 Provider 内 throw。
   修：挂原装 `WSProvider`（无 token 时不建连接，只提供空实现，见上）。
7. **mock 数据形状必须按原类型完整**（4 次详情页 crash，全是 mock 缺字段，不是原代码 bug）：
   - Issue 缺 `identifier` → 命令面板 `identifier.toLowerCase()` throw。修：mockIssue 按测试 fixture 补全（identifier / creator / parent / project / position / dates / metadata / properties / status_category）。
   - Project `status: "active"` 不在 `PROJECT_STATUS_CONFIG` → `config[status].dotColor` throw。修：`status: "planned"`。
   - Agent 缺 `skills` → Inspector `agent.skills.length` throw。修：补 `skills: []` 等。
   - Squad 缺 `leader_id` → 详情页 `leader_id.slice(0, 8)` throw。修：`leader_id: "a-claude"`。
   - Agents 页默认 scope 是 `mine`：mock agent 缺 `owner_id` 时 Mine 0 行、正确显示 No matches，不是 bug。修：一个 `owner_id: u-local`，一个 `owner_id: u-other`。
   定位方法：拿报错的 minified 堆栈行列号，从 `dist/assets/index-*.js` 抠出对应代码片段反查源码。

## Vendor 补丁（搬迁时对原代码的最小改动，都带标记）

1. `vendor/core/realtime/use-realtime-sync.ts`：2 处 `setQueryData` 直接传值 → 改为函数 updater（TanStack Query v5 Updater 推断问题，行为不变）。
2. `vendor/core/chat/mutations.ts`：1 处同类 `setQueryData` → 函数 updater。
3. 其余 vendor 文件**未改**。`frontend-a` 自己的 `tsconfig` 继承 `@multica/tsconfig/react-library.json`（lib ESNext），并补了 `@types/node` + `vendor/views/assets.d.ts`（svg/png 声明）。

## 后续批次

1. **传输层按页面迁移到 Rust**：mock handler 逐个接到 Rust 命令（`list_issues` 等已在 `src-tauri`，但返回 shape 与 core Issue 还有映射缺口，见 `tauri-api.ts` 的 `listIssues` 注释）；写操作（create/update/delete、mutation 类）优先接真实命令，读操作可先 mock。
2. **实时**：`WSClient` → Tauri `listen` 事件（run 日志、issue 变更、presence）。
3. **拆包**：`vite build` 输出拆包（manualChunks：mermaid/lowlight/tiptap 单独 chunk），主 bundle 约 6.3MB→拆分。
4. **删除清理**：analytics no-op、billing/invite/onboarding 云端分支、landing/营销页不进 Tauri。
5. **品牌/署名**：原仓库许可证对去品牌有限制，搬迁时保留 Multica 品牌/署名，除非另获授权（见 `docs/MIGRATION.md`）。

## 传输层接 Rust + 终验（2026-10-16）

- `frontend-a/src/tauri-api.ts` 重写：Tauri 环境下主要读写全部走真实 `invoke`，浏览器预览保留 mock。
- 实时：`frontend-a/src/components/tauri-realtime-bridge.tsx` 监听 `multica://entity-changed`，失效全部 React Query 缓存；`WSProvider` 仅作兼容上下文，不建连接。
- Vendor 补丁新增：`vendor/core/analytics/index.ts` → no-op（不初始化 PostHog、不发事件、不写 cookie，导出 API 不变）。
- 拆包：`vite.config.ts` manualChunks（mermaid / lowlight / tiptap / router / react-vendor / tanstack / motion / charts / vendor）。build 通过，主 chunk 4.3MB、vendor 12.6MB、mermaid 2.2MB（总 JS 约 21MB，懒加载需 views 侧 dynamic import，后续优化）。
- 终验结果：
  - `pnpm typecheck`：通过（0 error）。
  - `pnpm build`：通过（1m41s）。
  - `cargo check`：通过（0 error，仅 dead-code warning）。
  - 26 路由 Playwright crawl（浏览器 mock 模式）：0 API FAIL，0 页面错误。
  - **invoke 参数审计**：115 个 invoke 点与 121 个 Rust 命令签名逐个比对，0 不匹配。审计发现并修复：Rust 侧 `_session_id` 等下划线参数名与前端 `session_id` 对不上（Tauri 按名匹配会失败），已统一为无下划线名；修复过程中一次全局替换误伤 `*_id` → `*id`（如 `workspaceid`），已全部恢复并经 `cargo check` 验证。12 个 Rust 命令已注册但前端未调用（auth_*/start_run/cancel_run/list_runs/get_run/add_squad_member/remove_squad_member/assign_issue/toggle_autopilot/get_member），属预留能力，非 bug。
- 未能在沙箱验证：真实 Tauri GUI 运行时（沙箱缺 webkit2gtk，WebView 无法启动）。需在有 WebKit 的真机上跑一遍 smoke test：workspace/issue/comment/project/skill/agent CRUD + 事件桥接。

## 仍不完整（诚实边界）

见 `docs/MIGRATION.md` 的剩余缺口：chat 写路径、attachment 二进制、search snapshot、dashboard 统计聚合、project resources CRUD、issue property value CRUD、issue label 关联写、pins/views 写、inbox 写、squad 成员增删 transport、第三方频道配置、system wakeups、Agent Builder draft 持久化、真实 Git worktree、run 日志流式事件、其余 CLI 适配、Electron Updates 页签。
