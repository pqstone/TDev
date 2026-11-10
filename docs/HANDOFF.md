# TDev 项目交接文档（Agent Handoff）

> **本文档面向接手本项目的 AI Agent 与人类开发者**，完整承接项目历史、当前进度、关键决策与踩坑记录，使接手者无需依赖会话记忆即可无缝继续开发。
> 最后更新：2026-10-30。上手第一步：**先完整阅读 `AGENTS.md`**（最高开发规范），再读本交接文档与 `README.md`。

---

## 0. 最新状态（2026-11-10）

### 本轮新增（部署发布 + 多语言）

| 能力 | 位置 | 说明 |
| --- | --- | --- |
| 服务器管理 | `deploy_server_*`（lib.rs） | 按项目隔离；密码 **DPAPI** 加密，前端只拿 `has_password`；端口连通性测试 |
| 手动部署管道 | `src-tauri/src/deploy.rs` | 构建（自动加载 `Public/runtime/env.bat`、GBK 解码日志）→ 产物 glob → SSH/SFTP → 启动 → 启动验证 → 发布记录 |
| Agent 部署 | `deploy_agent_prepare` + 前端 deploy 场景 | 自动生成专用 SSH 密钥并写入 `authorized_keys`，把上下文和铁律交给 Agent，让它自己构建/上传/重启/验证 |
| 多语言 | `dist/index.html` i18n 区块 + `docs/i18n.md` | 中/英切换；原文即键的词典 + DOM 扫描 + MutationObserver；缺失文案采集器写 `app_meta.i18n_missing` |

### 目录/文件变化

- 新增 `src-tauri/src/deploy.rs`（部署引擎 + 命令）、`src-tauri/src/bin/probe.rs`（真机探针）
- 新增文档：`docs/deploy-module.md`、`docs/i18n.md`
- 前端仍为单文件 `dist/index.html`（i18n 区块在 `<script>` 开头）

### 验证方式

```bash
cd src-tauri
cargo test --lib                                  # 7 条单测
cargo run --release --bin probe                   # 完整部署管道（真实服务器）
cargo run --release --bin probe -- prepare        # 只验证 SSH 免密
cargo run --release --bin probe -- ssh "uname -a" # 服务器上执行命令
cargo build --release                             # 产物 target/release/tdev.exe
```

编译前必须杀掉正在运行的 `tdev.exe`：git-bash 里 `taskkill //F` 参数会被吃掉，用
`powershell -NoProfile -Command "Stop-Process -Name tdev -Force"`。

### 本机踩坑（已修，勿重犯）

1. 默认 `~/.ssh/id_ed25519` **带口令**，非交互 ssh 永远 Permission denied → 用专用密钥 `~/.ssh/tdev_deploy_ed25519`。
2. SFTP 必须先 `request_subsystem(true,"sftp")`；`SftpSession::write()` 不带 CREATE，必须 `create + write_all + close`。
3. SSH 的 `Eof` 之后才来 `ExitStatus`，在 Eof 就 break 会把成功误判为失败。
4. `#[tokio::test]` 会让 Tauri 的 test harness 链接出 `0xc0000139`，单测里不要用；真机验证走 `probe` 二进制。
5. Windows 控制台输出是 GBK，按 UTF-8 读会静默丢中文报错。

---

## 0.1 交接对象与阅读顺序

接手任何开发任务前，按此顺序读取权威文档（缺一不可）：

1. `AGENTS.md` — 项目最高开发规范（技术栈硬约束、命名、Tauri 规则、安全红线、开发工作流）。**必须逐条遵循，优先级最高。**
2. `docs/HANDOFF.md`（本文档）— 历史、进度、坑、待办。
3. `README.md` — 项目简介与主界面结构。
4. `UI-STYLE.md` + `theme/tokens.json` + `theme/ui-preview.html` — UI 风格唯一权威（北极冰山·深邃蓝）。
5. `docs/ARCHITECTURE.md` — 架构分层说明（注：尚未登记 SQLite 与最新界面演进，见 §9 待办）。

---

## 1. 项目概览

| 项 | 内容 |
| --- | --- |
| 项目名 | TDev（Tauri 桌面应用） |
| 项目根 | `E:\88.project\TDev`（属于 `E:\88.project` 大 git 仓库） |
| 定位 | 轻量、可扩展、结构清晰的 Tauri v2 桌面应用骨架，逐步扩展为带项目管理能力的开发工具 |
| 前端 | 纯 HTML/CSS/JS，**无框架**，成品维护在 `dist/`，Tauri 从 `../dist` 加载（`frontendDist`） |
| 后端 | Rust + Tauri v2，IPC 全部走自定义 `#[tauri::command]`，前端 `window.__TAURI__.core.invoke` |
| 本地数据库 | SQLite（`rusqlite`，内嵌免费开源），后端 command 封装 |
| UI 主题 | 北极冰山 · 深邃蓝（primary-700 `#0A4D8C`） |
| 目标 | 保障长期迭代/换 Agent 时项目结构清晰、架构规范、代码整洁低冗余 |

---

## 2. 技术栈与版本基线（已核实）

| 项 | 值 |
| --- | --- |
| Rust | 1.99.1（满足 Tauri ≥1.90） |
| cargo 路径 | `C:\Users\Administrator\.cargo\bin\cargo.exe` |
| Tauri | 2.x，配置 schema 2 |
| 依赖 | `tauri-plugin-fs="2"`、`tauri-plugin-shell="2"`、`rusqlite={version="0.32",features=["bundled"]}`、`uuid={version="1",features=["v4"]}`、serde、serde_json |
| 目标平台 | Windows（当前），结构上保持可移植 |
| 前端加载 | `tauri.conf.json` 中 `withGlobalTauri: true`（前端据此用 `window.__TAURI__`） |

> 新增任何第三方依赖必须先在 AGENTS.md 说明**用途/许可/体积/风险**再引入。

---

## 3. 目录结构（强制，不得擅自变更）

```
TDev/
├─ AGENTS.md              ← 最高开发规范（AI 必读）
├─ README.md              ← 项目简介（已同步最新界面/数据库）
├─ UI-STYLE.md            ← UI 风格规范（北极冰山·深邃蓝）
├─ theme/
│  ├─ tokens.json         ← 机器可读设计令牌（色值/间距/字号/圆角）
│  └─ ui-preview.html     ← 全组件视觉示意页
├─ dist/index.html        ← 前端成品（单文件内联 CSS/JS），Tauri 运行时加载
├─ src/                   ← 前端源码（模块化参考 main.js/styles.css；改动需同步 dist）
├─ docs/
│  ├─ ARCHITECTURE.md     ← 架构说明（待补充 SQLite/界面演进）
│  └─ HANDOFF.md          ← 本文档
└─ src-tauri/
   ├─ src/main.rs         ← 程序入口（黑窗子系统控制）
   ├─ src/lib.rs          ← 插件注册 + 全部 #[tauri::command]
   ├─ capabilities/default.json ← 权限（最小化：core:default, fs:default, shell:default）
   ├─ tauri.conf.json     ← 应用配置（identifier=com.tdev.app）
   ├─ Cargo.toml          ← Rust 依赖
   ├─ build.rs            ← Tauri 构建钩子（勿改）
   └─ icons/              ← 应用图标（自 E:\TauriTest 复制）
```

---

## 4. 历史演进时间线（完整承接）

| 阶段 | 内容 | 产物 |
| --- | --- | --- |
| ① 项目骨架 | 建立 Tauri v2 工程 + 根目录 `AGENTS.md`（11 节）+ README + `docs/ARCHITECTURE.md`；Debug 编译通过（17.59MB） | 目录骨架 |
| ② 主窗口三区 | 上中下三区：顶部标题栏（品牌+状态）、中部左右分栏（左=功能导航菜单、右=展示操作区）、底部标题栏（版本+状态） | `dist/index.html` 布局 |
| ③ 左侧双分类菜单 | 8 个菜单均配线性 SVG 图标（1.5px 描边、currentColor 跟随主题） | `dist/index.html` |
| ④ 北极冰山主题三件套 | `UI-STYLE.md`（11 章）+ `theme/tokens.json` + `theme/ui-preview.html`；主色 `#0A4D8C`，主色占比 ≤15%，色值一律取 tokens | UI 规范体系 |
| ⑤ 本地数据库接入 | 先推荐 SQLite → 用户确认"采用，接入并建库" → 接入 rusqlite | `tdev.db`、`db_init` 命令 |
| ⑥ 项目管理功能 | "项目工作"分类上方项目下拉条 + 「＋」新增项目弹窗 + `projects` 表 + `project_list`/`project_create` | 本轮新增，见 §5 |
| ⑦ 基础信息页 + 删除项目 | 菜单0展示选中项目完整信息（名称/类型/说明/prjGUID/创建时间），右下角删除按钮 + `project_delete` 命令 | 已闭环 |
| ⑧ 工具设置三功能 | 项目空间（工作路径+文件夹选择器）、模型配置（8提供商）、Agent配置；`cfg_save`/`cfg_load` 存 `app_meta`；引入 `tauri-plugin-dialog` | 已闭环 |
| ⑨ 新增项目目录生成 | 保存后工作空间建目录 + 复制 `src/template`（19文件）+ 模板文档重命名/占位符替换（template.md→{项目名}.md）+ 分步进度框 | 已闭环 |
| ⑩ 测试模块 + 在线聊天 | Agent连接测试页（QQ风格气泡），对接 Hermes API Server，`X-Hermes-Session-Id` 按 prjGUID 隔离会话 | 已闭环 |

---

## 5. 当前进度（截至 2026-10-30）

### 5.1 已完成并验证
- **主界面**：上中下三区布局、左侧双分类菜单（项目工作 5 项 + 工具设置 3 项）、8 个线性 SVG 图标，全部遵循北极冰山主题。
- **SQLite 接入（rusqlite）**：应用启动时前端 `invoke('db_init')` 建库建表，库文件 `%APPDATA%\com.tdev.app\tdev.db` 已生成。
  - **确定性验证**：python sqlite3 读库 → 表 = `app_meta`、`demo_items`、`projects`；`app_meta` 含 `theme=arctic-glacier-blue`。
- **项目管理功能（本轮）**：
  - 后端：`projects` 表（`prjGUID TEXT PRIMARY KEY, prjName, prjType, prjDescription, created_at`）；`project_list`（按创建时间倒序）、`project_create`（自动生成 UUID v4 作 prjGUID，参数化插入）。
  - 前端：菜单顶部 `[▼ 项目下拉框][＋]`；「＋」弹"新增项目"模态框（项目名称 / 项目类型：web · windows program · Tauri / 项目说明）；保存后刷新下拉框并回选新项目。
  - 编译通过（`cargo build --release`），应用已重启运行。
  - **修复一处参数名问题**：Tauri 命令参数在 IPC 层用 **camelCase**（Rust `prj_name` → 前端须传 `prjName`）。首次实现误传 `prj_name` 导致报错 `missing required key prjName`，已改为 `prjName/prjType/prjDescription` 并重编译。

### 5.2 进行中 / 未闭环（接手者注意）
- **无重大未闭环项**。GUI 实测落库已由用户确认通过；聊天功能全链路已实测。

### 5.3 后续待办（开放事项）
- 深色主题（北极冰山深色版）：按 AGENTS §6 约定在 `:root[data-theme="dark"]` 覆盖变量，不复制选择器。
- 项目下拉框选中后联动右侧内容（如加载该项目数据）未定义，待后续设计。
- 项目目录树展示 + 文档内嵌浏览插件（用户已规划，第二个子区域）。
- `src/` 源码与 `dist/` 成品同步（骨架阶段直接维护 dist，源码滞后）。

## 6. 关键事实与决策（ADR / 踩坑记录）

### 6.1 黑窗问题（已解决）
- **现象**：`E:\TauriTest` Debug exe 运行时除主窗口外还弹黑色命令行窗口。
- **根因**：`main.rs` 第 2 行 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` **只在 Release 生效**——Debug 构建带控制台（黑窗），Release 无黑窗。
- **决策**：演示/交付一律用 **Release**；Debug 保留控制台用于看日志。PE 头验证：Release Subsystem=2(GUI)、Debug=3(Console)。

### 6.2 SQLite 方案调整（关键决策）
- **`tauri-plugin-sql` 不可用**：其前端 API `window.__TAURI__.sql` 需要前端安装 npm 包 `@tauri-apps/plugin-sql`，而 TDev 是**纯静态无 npm** 项目，前端拿不到 sql 接口（GUI 实证），库建不起来。
- **改用 `rusqlite`（bundled 内嵌）**：免费开源单文件，后端封装 `db_init` 自定义命令，前端 `invoke` 调用——完全符合"纯静态前端 + 后端 command"架构。
- **Rust 修复链**：`tauri::api::path::app_config_dir` 不可用（需额外 feature）→ 改用 `.setup()` + `app.path().app_config_dir()`（`use tauri::Manager`），该函数返回 `Result` 非 `Option` 需 `?`。
- **构建被占用**：运行中的 exe 会占用 target 导致 `os error 5`，**编译前必须先 `Get-Process tdev | Stop-Process`**。

### 6.3 参数命名（本轮新坑）
- Tauri v2 command 参数在 IPC 层默认 **camelCase**：Rust 写 `prj_name`，前端 invoke 传参 key 必须是 `prjName`。返回 JSON 字段同理（`prjGUID/prjName/prjType/prjDescription`）。

---



### 6.4 Agent 聊天接入演进（重要）
- **PTY/TUI 方案（已废弃）**：/api/pty WebSocket 连接 TUI 界面，但 agent 不真正运行（界面假活、无回复），且 TUI 全屏重绘难以解析。
- **CLI 子进程方案（已废弃）**：`hermes -z --resume <sid>` 每次启动 python 子进程，有黑窗（`CREATE_NO_WINDOW` 可隐藏）、UI 假死（`spawn_blocking` 可解），会话隔离弱。
- **Hermes API Server（当前方案）**：HTTP `POST /v1/chat/completions` + `X-Hermes-Session-Id` header，按项目 prjGUID 隔离会话（验证通过：A 会话约定不被 B 会话看到）。
- **Origin 白名单**：Hermes web_server 的 `_LOOPBACK_HOST_VALUES` 需加入 `tauri.localhost`，否则 Tauri WebView 的 WS 被 origin_mismatch 拒绝。
- **会话 ID 陷阱**：`--resume` 传业务 ID（如 prjGUID）会报 session not found（TUI 模式）；API Server 的 `X-Hermes-Session-Id` 无此问题，可直接用 prjGUID。## 7. 数据库现状（SQLite / rusqlite）

- **库文件**：`C:\Users\Administrator\AppData\Roaming\com.tdev.app\tdev.db`（AppConfig 目录，setup 钩子自动建目录）。
- **表**：
  - `app_meta(key, value, updated_at)` — 键值配置，已有行 `theme=arctic-glacier-blue`。
  - `demo_items(id, name, status, created_at)` — 演示 CRUD。
  - `projects(prjGUID, prjName, prjType, prjDescription, created_at)` — 项目管理。
- **命令**：`db_init`（建库建表幂等）、`project_list`（列表）、`project_create`（新增，UUID v4，参数化插入）。
- **规范**：所有 DB 操作必须走 command + invoke；SQL 一律参数化绑定（`?`/`params!`）防注入；建表用 `CREATE TABLE IF NOT EXISTS`；新增表/字段先登记 AGENTS + docs。
- **查库验证手法**：写 `.py` 脚本用 python sqlite3 读表（**勿在 PowerShell 内联嵌套引号，会炸**）。库文件随应用启动自动创建。

---

## 8. 功能清单（后端命令 + 前端界面）

### 8.1 后端命令（`src-tauri/src/lib.rs`，已注册 generate_handler!）
| 命令 | 作用 | 备注 |
| --- | --- | --- |
| `create_folder(path)` | 递归新建文件夹 | 示例 |
| `run_script(script)` | cmd /C 执行脚本返回 stdout | **高风险**，演示用，生产须白名单化 |
| `db_init()` | 建库 + 建表骨架 + 写初始配置 | 返回库路径+表清单 |
| `project_list()` | 查询项目列表（创建时间倒序） | 下拉框数据源 |
| `project_create(prjName, prjType, prjDescription)` | 新增项目，自动生成 UUID v4 作 prjGUID | 参数化插入，返回 GUID |
| `project_delete(prjGuid)` | 按 prjGUID 删除项目 | 参数化，返回 bool |
| `cfg_save(category, payload)` | 配置 upsert 保存（工作空间/模型/Agent） | 存 `app_meta`，payload 为 JSON 字符串 |
| `cfg_load(category)` | 读取配置 | 返回 JSON 字符串或空 |
| `agent_ask(question, sessionId)` | 调用 Hermes API Server 问答 | **async + spawn_blocking**，HTTP `POST /v1/chat/completions`，`X-Hermes-Session-Id` 隔离会话 |

### 8.2 前端界面（`dist/index.html`）
- 三区布局 + 左侧**三分类**菜单（数据-m 0–8：项目工作 0-4、工具设置 5-7、测试模块 8，MODULES 对象，`render(idx)` 渲染右侧面板；5/6/7 为动态页 `workspace/models/agents`，8 为聊天页 `chat`）。
- **基础信息页（菜单0）**：选中项目后渲染 5 行信息（名称/类型/说明/prjGUID/创建时间）+ 右下角删除按钮。
- **工具设置动态页**：项目空间（`wsPath` 输入+浏览+保存）、模型配置（`mProvider` 8 提供商下拉+添加/删除）、Agent配置（`aType` 4 类型+添加/删除），数据存 `app_meta`。
- **聊天页（菜单8）**：QQ 风格气泡 + 状态徽标 + 下拉选 Agent；`chatAddMsg` 持久化历史（`chat_hist_` + sid），`chatSetPending` 持久化思考状态，切页恢复。
- **菜单顶部项目条**：`#projSel` 下拉框 + `#projAdd`「＋」按钮。
- **新增项目模态框**：`#projModal`（名称/类型/说明 + 取消/保存），样式按 UI-STYLE modal 规范（surface 底、radius-xl、shadow-lg、遮罩 `rgba(16,42,67,.45)`）。
- JS：`initDb()`（invoke db_init）、`loadProjects()`（invoke project_list 填充下拉框）、`saveProject()`（invoke project_create，成功刷新+回选+状态栏提示）。

---

## 9. 已知问题 / 风险 / 待办

1. **`run_script` 是任意命令执行**（AGENTS §8 红线）：默认关闭生产化，必须白名单/参数化后才能作为通用能力。
2. ~~ARCHITECTURE.md 滞后~~（已补齐 SQLite、新命令、聊天架构）。
3. ~~GUI 实测未闭环~~（用户已确认落库通过）。
4. **Hermes 依赖**：聊天依赖本机 Hermes API Server（127.0.0.1:8642，key `tdev-hulk-2026-api-server-key`），未启动时 `agent_ask` 报错。启动方式：Hermes 网关需带 api_server 平台配置。
5. **Tauri dist 嵌入**：改动 `dist/index.html` 后必须重新 `cargo build --release` 才生效（编译期嵌入）。
4. **cmd/shell 转义**：验证数据库时 PowerShell 内联 python 引号会出错，用 `.py` 文件再执行。
5. 主题图标均内联于 `dist/index.html`，改动主题须四步同步（tokens→UI-STYLE→示意页→标注）。

---

## 10. 接手 Agent 的起步步骤（推荐执行顺序）

1. 读 `AGENTS.md` 全文 → `docs/HANDOFF.md` → `README.md` → `UI-STYLE.md` + `theme/tokens.json`。
2. 读 `dist/index.html`（当前成品）与 `src-tauri/src/lib.rs`（当前命令）。
3. 验证环境：`cargo --version`、确认 `tdev.exe` 是否运行。
4. **若继续项目管理功能**：先完成 §5.2 的 GUI 实测落库确认；再考虑 §5.3 的联动/深色主题等开放项。
5. 任何改动遵循 AGENTS §9 工作流：探查→约束→证据→执行→验证→交付；改完 `cargo build --release` 并运行观察（先停进程）。
6. 更新 ARCHITECTURE.md 补齐 SQLite 与界面演进。

---

## 11. 相关周边项目（可能被引用）

| 项目 | 关系 |
| --- | --- |
| `E:\TauriTest` | 已验证可编译的 Tauri 对照组；TDev 的 icons 由此复制 |
| `E:\88.project\WMS`（即 `E:\V项目` 简易版 WMS） | 同一 `E:\88.project` 仓库的另一子项目（Java+Tomcat+MySQL，已交付）；涉及其文件时只读不改（AGENTS §11） |

> 提交规范见 AGENTS §10：`E:\88.project` 为共享 git 仓库，只动 TDev 范围内，`dist/`、`target/`、`node_modules/` 不提交。
