# Multica-Tauri 真机 Smoke Test（沙箱做不了的那部分）

沙箱缺 webkit2gtk，真实 Tauri GUI 无法启动，所以下面这些只能在真机（macOS / Windows / Linux 桌面）上跑。
目标：验证真实 `invoke` + SQLite + 事件桥接在真机上是通的，而不是只在浏览器 mock 模式下通。
沙箱已验证的（不用重测）：`pnpm typecheck`、`pnpm build`、`cargo check`、26 路由 Playwright 爬取 0 FAIL、115 个 invoke 点 vs 121 个 Rust 命令签名审计 0 不匹配。见 `docs/ROUTE_A.md`。

## 0. 启动

```bash
cd multica-tauri
pnpm install
cargo tauri dev
```

- macOS 用 WKWebView，不需要装 webkit2gtk（那是 Linux 沙箱缺的东西）。
- 首次启动会建库并 seed：工作区 `默认工作区`（slug `default`）、2 个 Agent（Claude / Codex，offline）、3 条示例 issue（MUL-1/2/3，分落在 Todo / In Progress / Backlog）。
- 数据库文件在 Tauri `app_data_dir` 下：macOS 约为 `~/Library/Application Support/ai.multica.local/multica.db`（identifier 见 `src-tauri/tauri.conf.json`）。如果路径对不上，以应用实际建库位置为准，报回实际路径即可。

## 1. 基础渲染（2 分钟）

- [ ] 应用打开不白屏，左侧栏完整（Issues / Projects / Autopilot / Agents / Squads / Skills / Runtimes / Analytics / Settings 等）。
- [ ] Issues 看板 5 列正常，3 条示例 issue 落在正确列（MUL-1 Todo、MUL-2 In Progress、MUL-3 Backlog）。

## 2. CRUD 往返（核心，每类都要走一遍）

每一步都不要手动刷新页面；写完后切到别的页面再切回来，确认数据在；最后重启应用一次，确认数据还在（SQLite 持久化）。

- [ ] **Issue**：新建 1 条（标题写 `smoke-<日期>`，如 `smoke-1016`）→ 看板出现 → 打开详情改标题/状态 → 列表同步变化 → 删除它。
- [ ] **Comment**：在任一 issue 详情里发 1 条评论 → 不刷新即出现 → 重启后还在。
- [ ] **Project**：Projects 页新建 1 个项目 → 列表出现 → 打开详情改名 → 删除。
- [ ] **Skill**：Skills 页新建 1 个 skill → 列表出现 → 删除。
- [ ] **Agent**：Agents 页新建 1 个 agent → 列表出现 → 删除。
- [ ] **Workspace**：确认默认工作区能正常切换/展示；如 UI 有新建工作区入口，新建 1 个空工作区 → 切过去是空看板 → 切回默认工作区数据还在。

## 3. 事件桥接（刷新路径）

Rust 写命令成功后会 emit `multica://entity-changed`，前端监听后全量失效 React Query 缓存（`frontend-a/src/components/tauri-realtime-bridge.tsx`）。
单窗口下它和 mutation 失效混在一起，无法单独隔离观测，所以按组合路径验收：

- [ ] 上面每类写操作后，**不点浏览器刷新**，相关列表/详情都已是最新值。
- [ ] 失败信号（任一出现即报）：写成功但列表还是旧数据，必须手动刷新才更新；或写完页面报错/白屏。

## 4. 本地优先（可选，但很有价值）

- [ ] 关 Wi-Fi（或断网）重启应用，上面任一读 + 写仍能正常工作 → 证明核心路径不依赖云端。
- [ ] 全程不应有发往 `telemetry.multica.ai` / PostHog 的请求（analytics 已是 no-op，源码不再 import `posthog-js`）。

## 5. 回报格式（直接贴回聊天）

按下面模板回，失败的贴出终端/控制台报错原文：

```
通过：1 / 2-Issue / 2-Comment / 2-Project / 2-Skill / 2-Agent / 2-Workspace / 3 / 4
失败：<步骤>，期望 <…>，实际 <…>，报错原文：<…>
DB 路径：<实际的 multica.db 路径>
```

## 已知不在本次 smoke 范围内（不用测）

chat 写路径、attachment 二进制、search snapshot、dashboard 统计聚合、project resources CRUD、issue property/label 写、pins/views/inbox 写、squad 成员增删 transport、频道配置、system wakeups、builder draft 持久化、真实 Git worktree、流式 run 日志、其余 CLI 适配、Updates 页签。见 `docs/MIGRATION.md` §7。
