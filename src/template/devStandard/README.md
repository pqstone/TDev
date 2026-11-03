# Web 开发标准模板（四层机制落地包）

> 给"开发类 Agent"使用：把工程标准从 Agent 的记忆中搬进**随库版本化的文件 + 自动化强制**，
> 确保长期开发中项目结构清晰、架构规范、代码整洁低冗余。

## 文件结构

```
web-dev-standard-template/
├── AGENTS.md                 # 【第一层·核心】Agent 开工必读：技术栈硬约束 + 目录 + 反模式 + 开工流程
├── ARCHITECTURE.md           # 【第一层】分层边界、依赖方向、数据库/Redis/高并发设计规范
├── CODING-STANDARDS.md       # 【第一层】Java/SQL/Redis/前端规范 + 交付自检清单
├── scaffold/                 # 【第二层】代码生成模板：DAO / Servlet / 前端页面（禁止从零画结构）
│   ├── README.md
│   ├── DaoTemplate.java
│   ├── ServletTemplate.java
│   └── WebPageTemplate.html
├── .pre-commit-config.yaml   # 【第三层】提交钩子：格式化+静态检查+SQL 守卫，不通过不让提交
├── .github/workflows/ci.yml  # 【第三层】CI：MySQL/Redis 服务 + 全量检查 + 打包 WAR
├── tools/sql_guard.py        # 【第三层】SQL 红线守卫：自动拦截 SELECT *
└── README.md（本文件）
```

## 应用到真实项目的步骤

1. **复制文件**：把 `AGENTS.md`、`ARCHITECTURE.md`、`CODING-STANDARDS.md`、`scaffold/`、`tools/` 拷入你的项目根目录。
2. **填空**：改 `AGENTS.md` 第 1 节（项目定位）、`pom.xml` 依赖版本、包名 `com.example`。
3. **启用提交钩子**（本地强制）：
   ```bash
   pip install pre-commit && pre-commit install
   ```
4. **启用 CI**：把 `.github/workflows/ci.yml` 放进你的 Git 仓库，PR 自动检查。
5. **示范一次**：让 Agent 先按 `scaffold/` 生成一个真实模块，对照规范走通，后续照此执行。

## 各层作用回顾

| 层 | 机制 | 靠什么保障 |
|---|---|---|
| 第一层 | 规范文件（AGENTS/ARCHITECTURE/CODING-STANDARDS） | 文件随库版本化，Agent 每次开工必读，不依赖记忆 |
| 第二层 | scaffold 模板 | 结构从模板生成，杜绝自由发挥 |
| 第三层 | pre-commit + CI + sql_guard | 工具硬拦截，违反就提交不进去 |
| 第四层 | 自检清单（CODING-STANDARDS 末尾） | Agent 交付前逐项核对 + 人工抽查统一标准 |

## 自定义要点

- 技术栈约束在 `AGENTS.md` 第 2 节集中维护，改动一处全局生效。
- 新增"禁止项"写入 `CODING-STANDARDS.md`（反模式生长，越用越严）。
- 样式类规范可并入 `STYLE-GUIDE.md` + `tokens.json`（与本包互不冲突）。
