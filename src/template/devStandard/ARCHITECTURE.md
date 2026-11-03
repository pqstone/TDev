# ARCHITECTURE.md —— 架构规范

> 定义分层边界、依赖方向与数据流。**与 AGENTS.md、CODING-STANDARDS.md 一起构成项目唯一权威规范。**

---

## 1. 总体分层

```
浏览器（静态 HTML/JS）
        │  XHR / WebSocket
        ▼
Controller 层（Servlet，src/.../controller）
        │  参数解析 · 调用 service · 统一 JSON 响应
        ▼
Service 层（src/.../service）
        │  业务逻辑 · 事务边界
        ▼
DAO 层（src/.../dao，唯一允许碰 SQL 的层）
        │  PreparedStatement · 按需取列
        ▼
MySQL（主存储）          Redis（高并发热点缓存）
```

**依赖方向（硬约束）**：只允许上层依赖下层。
- 禁止 Service 依赖 Controller；禁止 Controller 直接调 DAO；禁止 DAO 反向依赖。
- `model` / `util` / `redis` 为横向公共层，可被上层引用，但不得依赖业务层。

## 2. 各层职责（写死边界）

| 层 | 允许做 | 禁止做 |
|---|---|---|
| controller | 解析请求参数、校验入参、调 service、返回统一 JSON/页面 | 写 SQL、写业务规则、直接操作数据库 |
| service | 业务流程编排、事务管理、业务校验、调 dao/redis | 操作 HttpServletRequest/Response、直接碰 JDBC |
| dao | 拼 PreparedStatement、映射结果集、单表/多表查询 | 业务判断、事务管理（事务在 service 层控制） |
| redis | key 管理、读写缓存、TTL 设置 | 作为主存储（只做缓存） |
| ws | WebSocket 端点、会话管理与消息推送 | 业务逻辑（转发给 service） |
| util | 无状态公共方法（日期、字符串、分页） | 持有状态、依赖容器 |

## 3. 请求处理规范

1. 所有接口返回统一 JSON：`{"code":0,"message":"ok","data":...}`；`code!=0` 表示业务/系统错误。
2. Servlet 只做三件事：解析参数 → 调 service → 写响应。
3. 列表接口**必须支持分页**：入参 `page`/`size`，SQL 用 `LIMIT ?,?`。
4. 服务端异常统一在 controller 捕获并转成 JSON，禁止向客户端抛堆栈。
5. 静态资源走 `webapp/`，接口走 Servlet 映射，路径风格 `/api/xxx`。

## 4. 数据库设计规范（MySQL 8）

1. **命名**：库/表/字段全小写 snake_case；表名复数名词；字段不用保留字。
2. **主键**：`BIGINT` 自增或雪花 ID；禁止业务字段做主键。
3. **索引**：查询/过滤/排序字段必须建索引；联合索引遵循最左前缀；禁止无效冗余索引。
4. **非贪婪取列（强制）**：一条 SQL 只 select 当前功能用到的列；结果集按列名或下标精确取值。
5. **分页**：`LIMIT offset, size`（数据量大时用游标/键集分页），禁止无 limit 的整表查询。
6. **事务**：写操作（insert/update/delete 或涉及多表）必须在 service 层开启事务，异常回滚。
7. **建表脚本**：统一放 `sql/schema.sql`，变更追加新脚本（不修改历史脚本）。

## 5. Redis 使用规范

1. **定位**：只做高并发热点的缓存/限流/计数器，不做主存储、不做全量缓存。
2. **key 规范**：`业务域:对象:标识`，如 `user:info:1001`、`order:list:user:1001:page:1`。
3. **必须设置 TTL**；热点 key 要预估过期时间，避免雪崩（可加随机抖动）。
4. **缓存一致性（Cache Aside）**：读→先查缓存，未命中查库并回填；写→先更库，再删缓存。
5. **禁止**：大 key（如一次性塞入整个表）、无过期 key、缓存与库不一致的长事务窗口。
6. 客户端统一走 `redis/` 层封装，禁止散落各处 new 连接。

## 6. 高并发设计要点

1. 热点数据走 Redis 读缓存，降低 DB 压力。
2. 连接池（HikariCP 等）统一管理 DB 连接，禁止每次 new Connection。
3. 写热点可做异步削峰（队列 + 批量落库），但默认先保证正确性。
4. 所有系统间调用要有超时与降级，禁止无限等待。
