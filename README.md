# TDev — Tauri v2 桌面应用 / Tauri v2 Desktop Application

> **开放项目定位**：这是一个追求 **AI 规范化开发、工程化应用** 的开放项目。
> 项目以 `AGENTS.md` 为最高开发规范，驱动 AI Agent 按统一标准参与开发；
> 通过 devStandard 模板 + 业务流程建模 + 主题令牌体系，实现从需求到代码的**工程化落地**。
> 欢迎开放协作，共同探索 AI 驱动软件工程的最佳实践。


轻量桌面应用骨架：纯静态前端（HTML/CSS/JS）+ Rust（Tauri v2）后端，UI 采用**北极冰山 · 深邃蓝**主题。

- **前端**：纯 HTML/CSS/JS，无框架，成品维护在 `dist/`
- **后端**：Rust + Tauri v2，IPC 通过自定义 `#[tauri::command]`
- **交互**：前端 `window.__TAURI__.core.invoke` 调用后端命令；文件操作走 `tauri-plugin-fs`，外部命令走 `tauri-plugin-shell`
- **主题**：北极冰山 · 深邃蓝（`UI-STYLE.md` + `theme/`）
- **本地数据库**：SQLite（`rusqlite`，内嵌免费开源），后端 command 封装，前端 `invoke` 访问

## 当前主界面

上中下三区布局：

```
┌─────────────────────────────────────────┐
│ 顶部标题栏  ●TDev         主题/运行状态   │
├──────────────┬──────────────────────────┤
│ [▼ 项目下拉框][＋]│                          │
│ 项目工作      │                          │
│  ● 基础信息   │                          │
│    主题风格   │     右侧展示/操作区         │
│    项目约束   │     （随菜单切换内容）      │
│    业务流程   │                          │
│    部署发布   │                          │
│              │                          │
│ 工具设置      │                          │
│    项目空间   │                          │
│    模型配置   │                          │
│    Agent配置  │                          │
│ 测试模块      │                          │
│    Agent连接  │                          │
├──────────────┴──────────────────────────┤
│ 底部标题栏  TDev v0.1.0       ● 就绪      │
└─────────────────────────────────────────┘
```

- **菜单顶部为项目条**：项目下拉选择框（列出 SQLite `projects` 表已有项目）+ 「＋」按钮弹出**新增项目**弹窗（字段：项目名称 / 项目类型（web / windows program / Tauri）/ 项目说明；`prjGUID` 自动生成）。
- 左侧菜单为**三分类**，均带**线性 SVG 图标**（配合主题色）：
  - **项目工作**：基础信息、主题风格、项目约束、业务流程、部署发布
  - **工具设置**：项目空间（工作路径）、模型配置（大模型接入）、Agent配置（Hermes/OpenClaw 接入）
  - **测试模块**：Agent连接测试（QQ 风格在线聊天，对接 Hermes API Server）

## 已实现功能

- **基础信息页**：展示当前选中项目的基础信息（名称/类型/说明/prjGUID/创建时间），右下角删除项目按钮（confirm 确认后删除）。
- **新增项目全流程**：保存按钮显示分步进度（数据库→创建目录→复制模板→更新文档→生成说明），进度框异步不假死。
- **工具设置**：
  - **项目空间**：设定 TDev 工作路径（文件夹选择器），新建项目时在工作空间下创建项目目录。
  - **模型配置**：配置大模型接入（名称/提供商/URL/API Key/模型名），支持 OpenAI/DeepSeek/火山方舟/通义/Anthropic/Gemini/Ollama/自定义，Key 脱敏显示。
  - **Agent配置**：配置 Agent 接入（名称/类型/地址/Key/附加参数），支持 Hermes/OpenClaw/OpenAI Agent/自定义。
- **测试模块 → Agent连接测试**：QQ 风格在线聊天（气泡消息、输入框、发送按钮、连接状态），对接 Hermes API Server，X-Hermes-Session-Id 按项目隔离会话；消息历史 + 思考状态 localStorage 持久化。

## 快速开始

```powershell
# 安装 Rust 工具链后（本机路径：C:\Users\Administrator\.cargo\bin\cargo.exe）

# 编译并运行（Debug：保留控制台窗口便于看日志）
cargo build --manifest-path src-tauri/Cargo.toml
# 或运行
cd src-tauri; cargo run

# 发布构建（Release：无控制台黑窗，正式形态）
cd src-tauri; cargo build --release
# 产物：src-tauri\target\release\tdev.exe
```

## 目录结构

```
TDev/
├─ AGENTS.md             ← 面向 AI 开发的强制开发规范（优先读）
├─ README.md
├─ UI-STYLE.md           ← UI 风格、规范与约束（北极冰山·深邃蓝）
├─ theme/                ← 设计令牌与视觉示意
│  ├─ tokens.json        ← 机器可读设计令牌（色值/间距/字号/圆角）
│  └─ ui-preview.html    ← 组件视觉示意页（浏览器可预览）
├─ dist/                 ← 前端成品（Tauri 运行时加载目录，frontendDist=../dist）
├─ src/                  ← 前端源码（模块化参考，改后需同步到 dist）
├─ docs/ARCHITECTURE.md  ← 架构说明
└─ src-tauri/            ← Rust 后端
   ├─ src/main.rs        ← 程序入口（窗口子系统控制）
   ├─ src/lib.rs         ← 插件注册 + 自定义命令
   ├─ capabilities/      ← 权限（最小化）
   ├─ tauri.conf.json    ← 应用配置
   └─ Cargo.toml         ← Rust 依赖
```

## 本地数据库（SQLite）

- **方案**：`rusqlite`（免费开源、内嵌单文件）。不采用官方 `tauri-plugin-sql` 前端（其 `window.__TAURI__.sql` 需前端 npm 包，与本项目纯静态无 npm 冲突）。
- **库文件**：`%APPDATA%\com.tdev.app\tdev.db`（AppConfig 目录）。
- **访问方式**：前端 `invoke('db_init')` 等自定义 command；禁止前端直连或拼接 SQL。
- **已建表**：`app_meta`（键值配置）、`demo_items`（演示 CRUD）、`projects`（项目：prjGUID/prjName/prjType/prjDescription/created_at）。
- **项目相关命令**：`project_list`（列表）、`project_create`（新增，自动生成 UUID v4 作 prjGUID，同时在工作空间创建目录并复制 `src/template` 模板、生成项目文档）、`project_delete`（按 prjGUID 删除）。
- **配置命令**：`cfg_save`（配置 upsert 保存）、`cfg_load`（配置读取），用于工作空间/模型/Agent 配置持久化（`app_meta` 表）。
- **Agent 聊天命令**：`agent_ask(question, sessionId)`（异步，调用 Hermes API Server `/v1/chat/completions`，通过 `X-Hermes-Session-Id` header 按项目隔离会话）。

## 规范与主题

| 文件 | 用途 |
| --- | --- |
| `AGENTS.md` | 项目最高开发规范（AI 开发必读） |
| `UI-STYLE.md` | UI 风格、规范与约束（页面开发唯一风格依据） |
| `theme/tokens.json` | 机器可读设计令牌，色值一律由此引用 |
| `theme/ui-preview.html` | 全组件视觉示意页 |
| `docs/ARCHITECTURE.md` | 架构决策说明 |

---

# TDev - Tauri v2 Desktop Application (English Overview)

> Open Project Statement: TDev is an open project devoted to AI-driven standardized
> development and engineering-grade application practices. The project treats
> " + bt + "AGENTS.md" + bt + " as its authoritative development contract, guiding AI Agents
> to contribute under one unified standard. Through the devStandard templates,
> business-process modeling, and a theme-token system, it turns requirements into code
> in an engineering-first way. Open collaboration is welcome.

TDev is a Tauri v2 desktop application designed as an AI-collaborative engineering workspace:

- AI-Native Workflow: Agents (via Hermes API Server) generate themes, business-process flows, and project constraints from natural-language requirements.
- Standardized Development: " + bt + "AGENTS.md" + bt + " + devStandard templates enforce architecture, code style, and red lines for every AI contribution.
- Process Modeling: interactive flowcharts with unlimited nesting, node property editing, drag-and-drop restructuring, and snapshot backup/restore (SQLite).
- Tooling: workspace management, LLM model configuration, and Agent connection settings.
- Tech Stack: Pure static frontend (no framework) | Rust + Tauri v2 | SQLite (rusqlite).

### Modules

| Module | Function |
|---|---|
| Basic Info | Project info display + delete |
| Theme Studio | AI-generated theme tokens (palette / preview / save) |
| Project Constraints | devStandard templates, editable with auto-save |
| Business Process | AI flow modeling + infinite-level flowchart + node properties + drag-drop + backup |
| Tool Settings | Workspace / Model config / Agent config |
| Agent Chat | Hermes API Server chat with per-project session isolation |

### Milestones

- v0.1.0 - Skeleton + basic info + project management
- v0.2.0 - Tool settings (workspace / models / agents)
- v0.3.0 - Agent integration (CLI to API Server, session isolation)
- v0.4.0 - Business-process module (AI + infinite flowchart + properties + drag)
- v0.4.1 - Backup system (auto versions capped at 100 + snapshot backup/restore)
