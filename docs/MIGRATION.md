# Multica → Rust + Tauri：迁移映射（以原仓库 commit 8352787 为基准）

原仓库：`~/workspace/multica`（Go 后端 + Next.js Web + Electron + Expo，v0.6.2）
新仓库：本目录。目标：**功能与样式原样保留，去除云端相关功能，单机本地运行。**

## 1. 架构对照

| 原 Multica | 本地版（Rust + Tauri） | 说明 |
|---|---|---|
| Go 后端（Chi + sqlc + WebSocket） | Rust（Tauri commands + rusqlite + tokio） | REST + WS 全部改为 Tauri `invoke`，无 HTTP 端口、无 JWT |
| PostgreSQL 17（1196 个迁移） | SQLite（`multica.db`，1 个迁移） | 本地单机不需要 PG；如需多人 LAN，后续可加可选 PG（见 §5） |
| Next.js Web + Electron Desktop + Expo Mobile | Tauri 2（Webview 承载 React） | **前端不重写**：直接复用原 `packages/ui` + `packages/views` + `packages/core` 的 React 代码，只把 Next.js 路由/SSR 换成 Vite 静态 + Tauri；样式 token 原样复用 |
| Go Daemon（WebSocket 连后端，spawn CLI） | Tauri 进程内 Daemon（`src/daemon/`） | 进程内即 Daemon，无 WS 注册、无派单协议 |
| `server/pkg/agent`（26 个 CLI 适配器） | `src/daemon/mod.rs`（`cli_command` 映射） | 第一版只接 claude / codex，其余按优先级逐个移植 |
| Redis realtime relay | 删除，改内存广播 + 轮询 | 本地单机不需要 Redis |

> 关键取舍：**前端 React 代码是 50 万行，Rust 重写 UI（Dioxus / egui）不可能“样式原样保留”。**
> 唯一可行的“原样保留”是：React 前端原样搬进 Tauri Webview，Rust 只重写后端 + Daemon。
> 如果你坚持纯 Rust UI，请明确说，那意味着样式要重做，且工作量翻倍。

## 2. 删除的云端功能（完整清单）

| 删除项 | 原位置 | 替代 |
|---|---|---|
| Multica Cloud（`MULTICA_CLOUD_URL`、云端托管） | `.env.example`、`server/internal/cloudruntime/`、`cloud_pat.go` | 删除，本地即全部 |
| 计费 / 订阅（billing、entitlement、seatcapacity） | `server/internal/billing|entitlement|seatcapacity/`、`packages/core/billing/`、`packages/views/billing/`、`/billing` 路由 | 删除，本地无付费 |
| 遥测（telemetry.multica.ai，每日快照） | `server/internal/selfhosttelemetry/`、迁移 479–482 | 删除，零上报 |
| PostHog 分析 | `POSTHOG_*`、前端 posthog-js | 删除 |
| 邮件验证码（Resend / SMTP） | `RESEND_*`、`SMTP_*`、`server/internal/handler/email.go` | 删除；本地口令见 `src/auth.rs` |
| Google OAuth 登录 | `GOOGLE_CLIENT_*`、`auth/callback` | 删除 |
| S3 / CloudFront 附件 | `S3_*`、`CLOUDFRONT_*`、`server/internal/auth/cloudfront.go` | 改本地文件存储（`app_data_dir/uploads/`） |
| 90 天 PAT / Daemon JWT / Cloud PAT | `server/internal/auth/session.go`、`cloud_pat.go` | 删除，进程内调用无需 token |
| Landing / 营销页（homepage、pricing、contact-sales、usecases） | `apps/web/app/(landing)/` | 删除，Tauri 只留工作区 |
| 多租户 SaaS（邀请、公开实例、ALLOWED_EMAILS 域名白名单） | `apps/web/app/(auth)/invitations`、邀请路由 | 删除，本地单用户；如需共享工作区走文件导出/导入 |
| iOS / Android 移动端 | `apps/mobile/`（Expo） | 删除，Tauri 只做桌面（macOS / Windows / Linux） |

## 3. 保留的功能（按优先级迁移）

| P0（本骨架已做） | P1（下一批） | P2（后续） |
|---|---|---|
| Workspace、Issue 看板（5 列）、新建/改状态、指派 | Comment 时间线、Issue 详情（标签、依赖、附件） | 项目（Projects）、Squad、成员/权限 |
| Agent 列表/新建、Runtime 本机探测 | Agent 执行：后台线程 spawn CLI + 实时日志流 + 取消 | 26 个 CLI 逐个适配（codex 4409 行、hermes 3512 行是重头） |
| Run 记录、本地口令 | Chat（工作区问答）、Inbox 通知 | Autopilot 本地 cron 调度器、Webhook（仅本地/自填 URL） |
| Skill / Autopilot 表 + 查询命令 | Skill 导入/导出、执行时注入 skill bundle | VCS：GitHub/GitLab/Gitea/Forgejo 集成（保留，但改为用户自填 token，不走云端中转） |
| | 搜索、Usage 统计（本地计算） | 频道：Slack / Feishu / DingTalk / WeCom / Telegram（保留，但改为用户自填 webhook，不走云端中转） |

## 4. Go → Rust 关键映射

| Go | Rust |
|---|---|
| `chi` 路由 + handler（130 个文件） | `tauri::generate_handler!` + `commands/`（每个 handler → 一个 command） |
| `sqlc` 生成的查询 | `rusqlite` 手写 SQL（表少，sqlc 收益低） |
| `pgxpool` | `Mutex<Connection>`（单机够用；并发高时换 `r2d2_sqlite`） |
| `gorilla/websocket` + `realtime/hub.go` | Tauri `emit` 事件 + 前端轮询（P1 再做事件流） |
| `daemon.go` + `execenv/`（worktree、env、skill） | `daemon/`：worktree 用 `git worktree`、env 用 `std::process::Command` |
| `agent/*.go` 每个 CLI 的 prompt/参数/输出解析 | `cli_command(cli, workdir, prompt)` + 每 CLI 一个模块 |
| `migrations/*.up.sql`（600 次） | `migrations/001_init.sql`（本地核心表；如需导入旧数据，写一次性 PG→SQLite 迁移脚本） |
| `server/cmd/multica` CLI | 删除，Tauri 应用即 CLI；如需命令行，另加 `clap` 子命令 |

## 5. 待你确认的决策

1. **数据库**：本骨架用 SQLite（推荐，单机 Tauri 标准做法）。如需保留 Postgres（多人/已有数据），说一声，改成 `sqlx + postgres`，但 Tauri 安装就不再是“双击即用”。
2. **前端复用方式**：本骨架是“新写的小号 React（演示骨架）”。完整版有两条路——(A) 把原 `packages/ui + views + core` 整个搬过来（样式 100% 保留，工作量在适配 Next.js→Vite）；(B) 按骨架重写 UI（样式只能“接近”，不是原样）。推荐 A。
3. **VCS / 频道**：保留但改为“用户自填 token/webhook、直连第三方”，还是全部删除？本骨架默认保留接口、删除云端中转。
4. **许可证**：原 Multica License 禁止托管服务、禁止去品牌。重写后的代码是你的新实现，不受原 LICENSE 约束，但若直接复用原前端代码（路线 A），需遵守原 LICENSE 的署名/品牌条款。

## 6. 工作量诚实评估

原项目：Go 约 2036 文件 + TS/TSX 约 2466 文件，Go 测试 1214 + TS 测试 785，迁移 1196 个。
“全部功能原样保留”的重写不是一次会话能完成的。现实排期（1 人全职）：

- P0 骨架（本目录）：已交付，约 1 天等价工作
- 前端搬迁（路线 A）+ P1：约 2–4 周
- 26 个 CLI 适配 + Daemon 完整（worktree、日志流、取消、重试）：约 3–6 周
- P2（VCS、频道、Autopilot、权限）：约 2–4 周

本目录是可编译的起点（需本地有 Rust 工具链，沙箱无 cargo 故未在此编译验证），不是“已完成重写”。
下一步：确认 §5 的 4 个决策 → 按 P1 顺序逐批迁移，每批附测试。

## 7. 2026-10-16 进展（Route A 传输层 + 终验）

- 26 路由已接真实 Rust transport（读路径 + 主要写路径），Tauri 事件桥接已接，analytics 已 no-op，拆包已做。
- 终验：`pnpm typecheck` 通过、`pnpm build` 通过、`cargo check` 通过、26 路由 Playwright crawl 0 FAIL、invoke 参数审计 115 点 0 不匹配（见 `docs/ROUTE_A.md`）。
- 沙箱缺 webkit2gtk，真实 Tauri GUI 运行时尚未验证，需真机 smoke test。
- 仍不完整的写路径：chat 写、attachment 二进制、search snapshot、dashboard 统计聚合、project resources CRUD、issue property value CRUD、issue label 关联写、pins/views 写、inbox 写、squad 成员增删 transport、第三方频道配置、system wakeups、Agent Builder draft 持久化、真实 Git worktree、run 日志流式事件、其余 CLI 适配、Electron Updates 页签。`vendor/core/package.json` 仍声明 `posthog-js`（源码已不 import，依赖清理待做）。

## 8. 2026-10-16 进展（Batch 3 修复 + 终验）

PostHog 依赖已彻底清理：`vendor/core/package.json`、`pnpm-workspace.yaml` 的 catalog、`pnpm-lock.yaml`（catalogs / importers / packages / snapshots 四处，含带 peer 后缀的快照条目）0 残留，lockfile 仍可解析。vendor 里 posthog 只剩注释与配置 schema 字段，无实际调用。

Batch 3 写路径（`write3.rs`）按原后端逐项对齐并实测修复：

- **ZIP 导入**：支持 stored + deflate（flate2 原始 deflate 流），单条目 32 MiB 上限，加密条目拒绝。另修 EOCD 扫描 off-by-one（旧代码用 `..len-22`，把无注释 zip 的 EOCD 排除在外，正常 zip 全部报“找不到 EOCD”）。
- **技能 URL 导入**：GitHub blob URL 索引修正（`parts[2]=="blob"`、`parts[3]` 为分支）；`refresh_skill` 改用 `fetched_from`（raw 直链），不再拿 blob 页 HTML 当 SKILL.md。
- **cron 预览**：改用 chrono-tz（IANA 时区 + 完整 DST）：秋切重复小时触发两次、春切缺失小时跳过；未知时区报错不静默当 UTC；`5/15` 按标准语义 = `5-max/15`；输出截断到分钟。旧固定偏移实现把 Asia/Kolkata 近似为 +5（错半小时）且无 DST。
- **quick-create**：按原后端校验 agent_id/squad_id 二选一、priority 白名单、due_date 格式；squad 解析为 leader agent 并把指派写到 issue；project/parent 同工作区校验；attachment_ids 存在校验 + 绑定到新 issue。
- **分组 issues**：补齐全部原参数（status_categories、assignee_types、creator、metadata、properties、involves_user_id 四分支、assignee/creator_filters、project/label、日期窗口、group_assignee、sort_by/direction）；limit 默认 50 上限 100、每组分页（与原 ROW_NUMBER 窗口语义等价）；组顺序 member → agent → squad → 未指派（NULL 与 `''` 合并）。近似点：status 排序按字母序（原后端按工作区自定义状态顺序）、last_activity 按 updated_at（本地无 last_activity_at 列）、status_categories 直接匹配 issue.status_category、metadata 对象/数组值按精确相等。
- **归档箱**：按原 `inbox_archive.sql` 实现代表行去重（每 issue 一行）+ 同 issue 有未归档项则整组排除 + group_id + 游标分页；facets 为交叉过滤（每维度叠加其余三项）。修重名列 bug：`projected` 用显式列（`n.*` 自带 issue_status 会遮蔽 `i.status AS issue_status`）。
- **runtime 删除**：先真 kill 再解绑（旧顺序解绑后按 runtime_id 找不到 run，且只改 DB 不 kill 进程）；cancel 调 `cancel_run_by_id`（进程组 kill + cancelled 终态守卫）。
- **迁移 007**：already-applied 检测改为全部对象（4 表 + workspace.settings）；`ALTER ADD COLUMN settings` 改由 db.rs 的 has_column 守卫添加（007 文件本身全部幂等），避免 partial 迁移 + 版本未记录导致每次启动重试 ALTER 都失败。三种场景已验证：全新库、partial 007 补齐、settings 已存在但版本未记。
- **spawn_run**：UTF-8 安全截断（`String::truncate`/`s[..n]` 在非字符边界会 panic，watcher 线程死则 run 永远卡 running）；Unix 下 spawn 建新进程组、cancel 用 `kill(-pid, SIGKILL)` 连带杀 CLI 的子进程（失败回退单进程 kill）；DB 日志上限 1 MiB；错误文案补 copilot。
- **properties/metadata 过滤**：metadata 比较改 `CAST(json_extract(...) AS TEXT)`（`json_extract` 返回裸文本，`json('"alpha"')` 带引号，两边永不相等）；布尔值按 json_extract 的 1/0 转换。

验证：纯函数验证仓（`/tmp/rust-verify`，函数从 write3.rs 原样抽取）全部通过——cron 上海/加尔各答/纽约 DST 秋切双触发/春切跳过/未知时区报错/步长/秒截断、ZIP deflate+stored/data-descriptor/加密拒绝、skill-md、eq_candidates；SQL 镜像测试通过——迁移 007 三场景、归档箱去重/排除/游标/交叉 facets、分组过滤/排序/每组分页。终验：`cargo check --tests` 通过、`pnpm typecheck` 通过、`pnpm build` 通过、invoke 双向审计（289 调用点 / 280 命令，0 缺失，0 额外参数，仅 3 个本地锁屏预留命令未被前端调用）。

仍待真机验证：真实 Tauri GUI smoke test（沙箱缺 webkit2gtk）、真实 CLI run 的 kill/流式日志（需本机装 CLI）、真实 Git worktree 生命周期（当前为 `app_data/worktrees/<issue>` 隔离目录）。

频道 token 存储决策：`channel_installation.config` 仍明文存于 SQLite，未接系统 keychain。评估结论是本轮不做：本地库的全部数据（issues、评论等）都在同一 SQLite 明文文件里，信任 OS 用户账户边界；只把频道 token 移进 keychain 是边际收益，且 Linux Secret Service 在无密钥环环境会失败导致频道功能回归。后续若要做，需连同迁移、回退与真机 keychain 测试一起做。
