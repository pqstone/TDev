# AGENTS.md —— Agent 开工必读

> 本文件 + `ARCHITECTURE.md` + `CODING-STANDARDS.md` 是本项目**唯一权威规范**，随代码库版本化。
> **硬性约定：任何 Agent 在开始任何任务前，必须先完整读取上述三个文件**；任务中遇到写法不确定时，回到本文件确认，禁止凭个人习惯或"记忆里的通用做法"发挥。

---

## 1. 项目一句话定位

[在此填写项目简介，例如：面向内部运营的订单查询与数据看板系统]

## 2. 技术栈（硬约束，禁止引入未列出的技术）

| 层 | 规定 | 禁止 |
|---|---|---|
| 前端 | 纯 HTML + 原生 JavaScript(ES6) + WebSocket + 原生 XMLHttpRequest | Vue / React / jQuery / 任何 npm 依赖 / 任何构建工具 |
| 后端 | Java 17，Servlet + 手写分层 | Spring / SpringBoot / 任何重量级框架 |
| 服务器 | Tomcat 9+（WAR 部署） | 内置容器代替 Tomcat |
| 数据库 | MySQL 8，纯 JDBC（PreparedStatement）增删改查 | ORM / MyBatis / JPA / EntityManager |
| 缓存 | Redis（仅高并发热点场景） | 普通场景滥用 Redis |
| 构建 | Maven（依赖版本集中在 pom.xml） | Gradle / 手动管理 jar |

**新增任何技术依赖前，必须先说明理由并获得确认。**

## 3. 目录结构与职责（新代码必须按此落位）

```
项目根/
├── pom.xml
├── sql/                        # 建表/初始化脚本（版本化）
│   └── schema.sql
└── src/main/
    ├── java/com/example/
    │   ├── controller/         # Servlet：只做参数解析与响应，不写业务
    │   ├── service/            # 业务逻辑与事务边界
    │   ├── dao/                # 纯 JDBC 数据访问（唯一允许碰 SQL 的层）
    │   ├── model/              # 数据传输/值对象
    │   ├── redis/              # Redis 客户端封装与 key 管理
    │   ├── ws/                 # WebSocket 端点
    │   └── util/               # 无状态工具类（禁止有状态单例）
    └── webapp/
        ├── index.html          # 静态页面（纯 HTML/JS/CSS）
        ├── js/
        └── css/
```

**代码新增/修改必须落在对应职责目录，禁止跨层越权。**

## 4. 核心约定（必须遵守）

1. **按需取列**：SQL 只用当前功能需要的字段，禁止 `SELECT *`。
2. **参数化查询**：所有 SQL 必须用 `PreparedStatement` 占位符，禁止字符串拼接 SQL。
3. **分层依赖单向**：`controller → service → dao`，禁止反向/跨层调用。
4. **JSON 统一格式**：所有接口响应 `{ "code": 0, "message": "ok", "data": ... }`。
5. **资源必释放**：连接/Statement/ResultSet 用 `try-with-resources`。
6. **Redis 必设过期时间**，key 遵循 `业务域:对象:ID` 规范。
7. **前端零依赖**：不引第三方库；XHR 与 WebSocket 用原生 API 封装。
8. **新代码照模板**：新增模块必须基于 `scaffold/` 模板生成，不从零画结构。

## 5. 反模式速查（禁止，违反即返工）

- ❌ `SELECT *`、`SELECT 全字段`（用多少读多少）
- ❌ 字符串拼接 SQL（注入风险）
- ❌ Controller 里写 SQL / 业务逻辑
- ❌ Service 里操作 HttpServletRequest / 直接碰 JDBC
- ❌ 引入 ORM、前端框架、任何未确认的新依赖
- ❌ 复制粘贴重复代码（抽公共方法/工具类）
- ❌ 吞异常（`catch (Exception e) {}` 空捕获）
- ❌ Redis 不设 TTL、缓存全表数据
- ❌ 方法超过 40 行不做拆分

## 6. 开工流程（每个任务必须按此执行）

1. 读取本文件 + `ARCHITECTURE.md` + `CODING-STANDARDS.md`。
2. 阅读相关现有代码，保持既有风格与命名。
3. 按 `scaffold/` 模板生成新代码。
4. 交付前逐项跑完 `CODING-STANDARDS.md` 末尾的**自检清单**。
