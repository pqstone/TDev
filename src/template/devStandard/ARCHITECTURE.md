# ARCHITECTURE.md —— 架构规范

> 定义分层边界、依赖方向与数据流。**与 AGENTS.md、CODING-STANDARDS.md 一起构成项目唯一权威规范。**
>
> **文档地图**
> - **第 1~6 章**：分层架构与各层硬边界（系统怎么搭、每层能做什么）
> - **第 7~13 章**：预留式 MVP 交付规范（功能可以精简，但数据地基与访问入口必须一次做对）
> - **第 14 章**：变更记录
>
> 三份文档职责不重叠：AGENTS.md = 目录结构 + 开工流程；本文件 = 分层与架构约束；CODING-STANDARDS.md = 代码写法 + 通用自检清单。

---

## 1. 总体分层

```
浏览器（静态 HTML/JS）
        │  XHR / WebSocket
        ▼
AuthFilter（Servlet Filter · 请求入口统一关卡）
        │  注入 UserContext，免登录时直接放行（见 9.3）
        ▼
Controller 层（Servlet，src/.../controller）
        │  参数解析 · 权限守卫 can() · 调用 service · 统一 JSON 响应
        ▼
Service 层（src/.../service）
        │  业务逻辑 · 权限实现 · 事务边界
        ▼
DAO 层（src/.../dao，唯一允许碰 SQL 的层）
        │  PreparedStatement · 按需取列 · 统一拼 DataScope 过滤（见 8.2）
        ▼
MySQL（主存储）          Redis（高并发热点缓存）
```

**依赖方向（硬约束）**：只允许上层依赖下层。

- 禁止 Service 依赖 Controller；禁止 Controller 直接调 DAO；禁止 DAO 反向依赖。
- `AuthFilter` 只依赖 `UserContext` / `AuthConfig`，禁止反向依赖业务层。
- `model` / `util` / `redis` 为横向公共层，可被上层引用，但不得依赖业务层。

## 2. 各层职责（写死边界）

| 层 | 允许做 | 禁止做 |
|---|---|---|
| controller | 解析请求参数、校验入参、**权限守卫 `can()`**、调 service、返回统一 JSON/页面；`AuthFilter` 属本层 | 写 SQL、写业务规则、直接操作数据库 |
| service | 业务流程编排、事务管理、业务校验、**权限实现**、调 dao/redis | 操作 HttpServletRequest/Response、直接碰 JDBC |
| dao | 拼 PreparedStatement、映射结果集、单表/多表查询、**统一拼 `DataScope.clause()`** | 业务判断、事务管理（事务在 service 层控制） |
| redis | key 管理、读写缓存、TTL 设置 | 作为主存储（只做缓存） |
| ws | WebSocket 端点、会话管理与消息推送 | 业务逻辑（转发给 service） |
| model | 数据实体（POJO）、枚举、常量；`User` 等身份模型在此 | 写业务逻辑、碰 JDBC / Servlet |
| util | 无状态公共方法（日期、字符串、分页、`DataScope`）。**唯一例外**：`UserContext` / `AuthConfig` 是框架级工具（请求级上下文与全局常量），不受"无状态"约束 | 持有**业务**状态、依赖容器 |

**边界澄清（易争议的两点）**：

- `can()` 调用在 controller，是**入口守卫**（拦不拦这个请求），不是业务规则；**能不能做这件事**的判定实现在 `PermissionService`（service 层）。
- `UserContext` 用 `ThreadLocal` 持有**当前请求的身份**，属于框架机制，不违反 util"无状态"原则——它不跨请求、不缓存业务数据，请求结束即 `clear()`。禁止在 `UserContext` 里塞任何业务字段。

## 3. 请求处理规范

1. 所有接口返回统一 JSON：`{"code":0,"message":"ok","data":...}`；`code!=0` 表示业务/系统错误。
2. Servlet 只做三件事：解析参数 → 调 service → 写响应。
3. 列表接口**必须支持分页**（入参 `page`/`size`），SQL 分页写法见 CODING-STANDARDS.md 2.2。
4. 服务端异常统一在 controller 捕获并转成 JSON，禁止向客户端抛堆栈。
5. 静态资源走 `webapp/`，接口走 Servlet 映射，路径风格 `/api/xxx`。
6. 业务异常统一抛 `BizException(code, messageKey)`，由 controller 捕获转 JSON；`messageKey` 走文案体系（见 8.4），**禁止在异常消息里写死中文**。

## 4. 数据库设计规范（MySQL 8）

1. **命名**：库/表/字段全小写 snake_case；表名复数名词；字段不用保留字。
2. **主键**：`BIGINT` 自增或雪花 ID；禁止业务字段做主键。
3. **索引**：查询/过滤/排序字段必须建索引；联合索引遵循最左前缀；禁止无效冗余索引。
4. **查询写法**：非贪婪取列、分页、参数化统一见 CODING-STANDARDS.md 2.1 / 2.2，**本文不重复**。
5. **事务归属（架构约束）**：写操作必须在 **service 层**开启事务，DAO 不得自管事务。
6. **建表脚本**：统一放 `sql/schema.sql`，变更追加新脚本（不修改历史脚本）。
7. **四尾字段（强制）**：所有业务表必须含 `id` / `created_at` / `created_by` / `org_id`，定义、默认值与索引要求见 **8.1**。

## 5. Redis 使用规范

**定位（架构约束）**：Redis 只做高并发热点的缓存 / 限流 / 计数器，**不做主存储、不做全量缓存**。

key 命名、TTL 强制要求、Cache Aside 读写顺序、禁大 key、统一走 `redis/` 封装等**具体写法**，
唯一来源 = `CODING-STANDARDS.md` 第 3 节（本文不重复）。

## 6. 高并发设计要点

1. 热点数据走 Redis 读缓存，降低 DB 压力。
2. 连接池统一管理 DB 连接，禁止每次 new Connection（写法见 CODING-STANDARDS.md 1.5）。
3. 写热点可做异步削峰（队列 + 批量落库），但默认先保证正确性。
4. 所有系统间调用要有超时与降级，禁止无限等待。

---

## 7. 交付策略：预留式 MVP

> **适用场景**：第一版只做业务流程，登录、权限、多语言、主题等基础功能延后交付。
> 这在产品上通常是对的（先让用户看到完整系统，再决定要不要复杂权限），但架构上有三类风险：

| 风险 | 后果 |
|---|---|
| 数据表缺少归属字段 | 后期加权限要逐表加列、洗数据、改所有查询 |
| 代码里没有权限检查点 | 后期只能散弹式打补丁，漏一处就是漏洞 |
| 文字与颜色写死 | 后期支持多语言 / 主题要全局替换 |

**本节目的：用不到 10% 的额外成本，把后期改造从"伤筋动骨"降级为"机械施工"。**

### 7.1 一句话原则

> **功能可以后补，数据结构和访问入口不能后补。**
> **前者是装修，后者是地基。**

### 7.2 三类改动分级（先归类，再决定做不做）

| 类型 | 特征 | 例子 | 后期成本 | 处理方式 |
|---|---|---|---|---|
| **A 类 零成本预防** | 现在做只多几行 | 建表带四尾字段、`api.js` 统一封装、CSS 变量取色 | ≈ 0 | **第一版必须做** |
| **B 类 机械式补装** | 模式化、可批量 | 查询加 `org_id` 过滤、补 `can()` 实现 | 中 | **允许后补** |
| **C 类 结构性重写** | 改动面横跨全项目 | 改主键、拆表、改鉴权入口、改路由架构 | 高 | **第一版必须避免** |

**判断口诀**：这件事如果后补，需要改"一个地方"还是"所有地方"？

- 改一个地方 → A 类，**现在就做**
- 改所有地方但模式相同 → B 类，**可以后补**
- 改所有地方且模式不同 → C 类，**现在就要留位置**

### 7.3 与 CODING-STANDARDS「YAGNI」的边界（重要，避免误判）

CODING-STANDARDS 第 5 节要求 **YAGNI：只写当前需要的代码，禁止为"未来可能"堆砌无用抽象**。
第 8 章要求**必须预留四个接缝**。二者看似冲突，实际不冲突，判定标准如下：

| | YAGNI 禁止的 | 接缝允许的 |
|---|---|---|
| 本质 | **未实现的功能**与**抽象层**（接口、工厂、继承体系） | **零逻辑占位**：调用点 + 恒真实现 |
| 特征 | 多出类、多出层次、多出配置项 | 只有一行 `return true` / `return ""` / 一个常量 |
| 判断 | 用户看不到它的价值，只增加理解成本 | 不改行为，只改"以后改哪里" |

**一句话**：**只留"调用点 + 恒真实现"，不写"未来功能的空壳"。**
凡是需要新建抽象类、接口、策略模式的"预留"，都是 YAGNI 范围，**不做**。

---

## 8. 四个接缝（第一版必须落地）

四个接缝是后期改造的"收口点"。**第一版不实现任何功能，只保证有地方可改。**

### 8.1 接缝一：数据表四条尾巴

所有业务表（**无例外**）必须含四列，定义见下表（强制要求已写入第 4 章第 7 条）：

| 列 | 类型 | 默认值 | 作用 |
|---|---|---|---|
| `id` | `BIGINT` 自增 | — | 主键 |
| `created_at` | `DATETIME` | `CURRENT_TIMESTAMP` | 审计 |
| `created_by` | `VARCHAR(64)` | `'local'` | 数据归属 |
| `org_id` | `BIGINT` | `1` | 组织 / 租户隔离 |

**建表示例**（关键是 `DEFAULT`：第一版 DAO 的 INSERT 完全不用关心这四列）：

```sql
CREATE TABLE orders (
  id          BIGINT       NOT NULL AUTO_INCREMENT,
  order_no    VARCHAR(64)  NOT NULL,
  amount      DECIMAL(12,2) NOT NULL DEFAULT 0,
  status      TINYINT      NOT NULL DEFAULT 0,
  created_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by  VARCHAR(64)  NOT NULL DEFAULT 'local',
  org_id      BIGINT       NOT NULL DEFAULT 1,
  PRIMARY KEY (id),
  KEY idx_org_created (org_id, created_at),
  KEY idx_order_no (order_no)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

**规则**：

1. 第一版**不需要**任何 Java 代码读写 `org_id`，但**列必须存在**。
2. `org_id` **必须进联合索引首列**，否则后期加过滤条件会全表扫描。
3. 禁止出现"先不加、以后再说"的业务表。
4. 已有表缺四尾的，按第 4 章第 7 条在 `sql/` 追加变更脚本补齐。

### 8.2 接缝二：统一数据访问（前后端各一个入口）

**后端**：DAO 层已是唯一允许碰 SQL 的层（见第 1、2 章）。在此基础上加一个**数据范围助手**，第一版返回空条件：

```java
// com/example/util/DataScope.java
public final class DataScope {
    private DataScope() {}

    /** 第一版：不加任何条件。后期实现：追加 " AND org_id = ?" */
    public static String clause() {
        return "";
    }

    /** 第一版：无参数可绑。后期按 clause() 的条件依次绑定 */
    public static int bind(PreparedStatement ps, int index) throws SQLException {
        return index;   // 返回下一个可用的占位符下标
    }
}
```

DAO 里的用法（**所有列表 / 详情查询统一这么写**）：

```java
String sql = "SELECT id, order_no, amount, status FROM orders WHERE status = ?"
           + DataScope.clause()
           + " ORDER BY id DESC LIMIT ?, ?";
try (PreparedStatement ps = conn.prepareStatement(sql)) {
    ps.setInt(1, status);
    int idx = DataScope.bind(ps, 2);      // 第一版什么都不做
    ps.setInt(idx,     (page - 1) * size);
    ps.setInt(idx + 1, size);
    // ... 执行与映射
}
```

**前端**：页面不裸写 XHR，统一走 `webapp/js/api.js`（CODING-STANDARDS 第 4 节已有此约定）。
所有请求、错误码处理、带 token 的钩子都在这一个文件里，后期加鉴权**只改这里**。

**禁止**：

- 页面里直接 `new XMLHttpRequest()`
- DAO 里手写不带 `DataScope.clause()` 的列表查询

### 8.3 接缝三：PermissionService.can()（恒为 true）

```java
// com/example/service/PermissionService.java
public class PermissionService {

    /** 第一版：永远放行。后期在此实现角色 / 权限判断 */
    public boolean can(String action, String resource) {
        return true;
    }
}
```

**关键不在实现，在调用点已经埋好**。Controller 的主操作入口统一加：

```java
if (!permissionService.can("order:edit", "order")) {
    throw new BizException(403, "msg.no.permission");   // 文案键，见 8.4
}
```

**覆盖范围**：只埋在**主操作入口**（新增 / 编辑 / 删除 / 审批），不要每个 getter 都埋——过度会污染代码。
第一版这些判断永远通过，**看起来是废代码，其实是后期的插槽**（边界见 7.3）。

### 8.4 接缝四：文案、颜色、菜单

| 项 | 第一版做法 | 后期 |
|---|---|---|
| 前端文案 | 统一 `t('key')`，即使只有中文一套 | 加 `en.js` 语言包 |
| 后端文案 | 错误提示走 `BizException` + 文案键（`messages.properties`） | 加 `messages_en.properties` |
| 颜色 | 全部用 CSS 变量 `var(--color-primary)`，集中在 `:root` | 新增一组变量覆盖 |
| 菜单 | 由 `webapp/js/menu.js` 配置数组渲染，不写死在 HTML | 加 `visible` 规则 |

```js
// webapp/js/i18n.js
var ZH = { 'save': '保存', 'cancel': '取消', 'msg.no.permission': '没有操作权限' };
function t(key) { return ZH[key] || key; }
```

**这三样不做，纯粹是省事，不是简化。**

---

## 9. 认证架构预留（第一版明确"不要登录"时的标准做法）

### 9.1 核心原则

> **预留的是接缝，不是功能。**

**错误做法**：做一个"会自动跳过的登录页"——用户会看到闪屏、产生困惑，代码里还留着死逻辑。

**正确做法**：用户完全感知不到预留的存在，但"现在是谁在操作"这个问题永远有答案。

### 9.2 第 1 层：UserContext —— 全项目唯一身份出口

```java
// com/example/model/User.java
public class User {
    private Long id;
    private String name;
    private String role;     // admin / staff / ...
    private Long orgId;
    // getter / setter 省略
}

// com/example/util/UserContext.java
public final class UserContext {
    private static final ThreadLocal<User> HOLDER = new ThreadLocal<>();

    /** 第一版：免登录，恒返回一个完整的默认用户。
     *  这是全项目唯一允许出现身份字面量的地方。 */
    private static User defaultUser() {
        User u = new User();
        u.setId(1L);
        u.setName("本机用户");
        u.setRole("admin");
        u.setOrgId(1L);
        return u;
    }

    public static User current() {
        User u = HOLDER.get();
        return u != null ? u : defaultUser();
    }

    public static void set(User u) { HOLDER.set(u); }

    /** Filter 的 finally 里必须调用，防线程池串号 */
    public static void clear() { HOLDER.remove(); }
}
```

**铁律**：

- 业务代码需要身份时，**只能**调用 `UserContext.current()`
- 禁止出现写死的用户名、`"admin"`、`"当前用户"` 等身份字面量
  —— **唯一例外**：`UserContext.defaultUser()` 内部（第一版默认用户在此定义，别处一律禁止）
- 禁止在业务代码里判断"有没有登录"

**为什么用 ThreadLocal**：Servlet 线程模型下每个请求一个线程。登录后由 Filter 在请求入口 `set`，整条调用链（Controller → Service → DAO）都能取到，不用层层传参。第一版它只是个恒返回默认值的壳。
**它不违反第 2 章 util 的无状态要求**：上下文随请求生灭、请求结束即清空，不跨请求、不存业务数据。

### 9.3 第 2 层：AuthFilter —— 一个空过滤器

```java
// com/example/controller/AuthFilter.java
@WebFilter("/*")
public class AuthFilter implements Filter {

    @Override
    public void doFilter(ServletRequest req, ServletResponse res, FilterChain chain)
            throws IOException, ServletException {
        try {
            if ("none".equals(AuthConfig.AUTH_MODE)) {
                chain.doFilter(req, res);   // 第一版：直接放行，不做任何校验
                return;
            }
            // 后期：校验 session / token
            // User user = (User) ((HttpServletRequest) req).getSession().getAttribute("user");
            // if (user == null) { 返回 401 JSON 或跳登录; return; }
            // UserContext.set(user);
            chain.doFilter(req, res);
        } finally {
            UserContext.clear();            // 防止线程复用串号
        }
    }
}
```

**为什么必须有这道门**：没有它，后期只能在每个 Servlet 里手工加校验，散弹式改动、必漏。
有了它，**一处生效，全站通吃**。

> **落位说明**：Filter 属 Web 入口层，放 `controller` 包（AGENTS.md 目录结构已支持）。
>
> **后期启用登录时的必做项**：`@WebFilter("/*")` 会拦所有路径，必须在 Filter 里**白名单放行** `/login.html`、`/api/login`、`/js/**`、`/css/**`、`/img/**`，否则登录页自身被拦截会造成**无限重定向**。

### 9.4 第 3 层：数据归属 —— 字段先占位

见 8.1。第一版靠 MySQL 的 `DEFAULT` 值自动填充，**DAO 的 INSERT 语句不用关心**。
即：第一版不需要写任何"填充 created_by"的代码。

### 9.5 AUTH_MODE 开关

```java
// com/example/util/AuthConfig.java
public final class AuthConfig {
    /** none = 免登录 | simple = 单账号密码 | full = 多用户 + 角色 + 权限 */
    public static final String AUTH_MODE = "none";
}
```

| 值 | 含义 | 用户感知 |
|---|---|---|
| `none` | 免登录（第一版默认） | 无 |
| `simple` | 单账号密码 | 一个登录页 |
| `full` | 多用户 + 角色 + 权限 | 登录 + 用户管理 |

**升级 = 改一个字符串 + 补实现**，不是架构改造。

### 9.6 升级为登录系统：只需改 4 处

| 序号 | 文件 | 改动 |
|---|---|---|
| ① | `util/AuthConfig.java` | `AUTH_MODE` 改为 `"full"` |
| ② | `controller/AuthFilter.java` | 补 session / token 校验 + 白名单 + `UserContext.set(user)` |
| ③ | 新增 `LoginServlet` + `login.html` | 登录页与登录接口 |
| ④ | `sql/` 新增脚本 | 新建 `users` 表（免登录期间**不存在此表**） |

**业务代码：0 改动。**

```sql
-- 升级为登录系统时才执行，免登录期间不建
CREATE TABLE users (
  id          BIGINT       NOT NULL AUTO_INCREMENT,
  username    VARCHAR(64)  NOT NULL,
  password    VARCHAR(128) NOT NULL,     -- 存哈希，禁止明文
  name        VARCHAR(64)  NOT NULL,
  role        VARCHAR(32)  NOT NULL DEFAULT 'staff',
  org_id      BIGINT       NOT NULL DEFAULT 1,
  created_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by  VARCHAR(64)  NOT NULL DEFAULT 'local',
  PRIMARY KEY (id),
  UNIQUE KEY uk_username (username)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

### 9.7 历史数据归属规则（必须写死）

免登录运行期间产生的数据，`created_by` 全部为 `'local'`。
升级登录后这些数据归谁？

> **规则：全部归属第一个被创建的管理员账号。**

升级时必须执行一次（每张含 `created_by` 的业务表）：

```sql
UPDATE orders    SET created_by = ? WHERE created_by = 'local';
UPDATE customers SET created_by = ? WHERE created_by = 'local';
-- 其余业务表同理
```

**不写清这条规则的后果**：Agent 会随意处理，用户升级后"看不到自己以前的数据"，会认为数据丢失。

### 9.8 深链接回归

免登录时用户可能收藏了具体页面地址（如 `/order.html?id=123`）。
升级登录后未登录访问 → 跳登录页 → **登录成功必须回到原地址**，不能回首页。

`api.js` 里收到 401 的统一处理，第一版就把 `redirect` 参数位留好：

```js
function gotoLogin() {
  var back = encodeURIComponent(location.pathname + location.search);
  location.href = '/login.html?redirect=' + back;   // 参数先传，第一版走不到这
}
```

---

## 10. AI 编码铁律，以下行为一律禁止：

1. 在业务代码中硬编码用户登录、身份信息
   —— **唯一例外：`UserContext.defaultUser()`**
2. 页面直接 `new XMLHttpRequest()`，绕过 `api.js`（前端写法见 CODING-STANDARDS.md 4.3）
3. DAO 列表 / 详情查询不带 `DataScope.clause()`
4. 创建不含 `id / created_at / created_by / org_id` 的业务表
5. 在 HTML / JS 中写死界面文字（必须走 `t()`）
6. 在 CSS / 内联样式中写死颜色值（必须用变量）
7. 删除或跳过 `permissionService.can()` 调用点（即使它永远返回 true）
8. 一次性改动超过一个模块（重构必须分步）
9. 在无代码备份 / 无版本回退能力时进行结构性重构

---

## 11. 第一版交付自检清单（预留式 MVP 专项）

> **与 CODING-STANDARDS.md 第 6 节清单并行执行**：那份覆盖 Java / SQL / Redis / 前端通用项，本清单**只列预留式 MVP 增量项**，不重复。

**数据地基**

- [ ] 所有业务表含 `id / created_at / created_by / org_id`，且四列有 DEFAULT
- [ ] 每张表的 `org_id` 进入联合索引首列
- [ ] 建表 / 变更脚本已放入 `sql/`（含四尾与索引）

**访问入口**

- [ ] 所有列表 / 详情查询都拼了 `DataScope.clause()`
- [ ] 前端全部走 `api.js` 统一封装（无裸 XHR）
- [ ] 全局搜 `"admin"` / 中文用户名，除 `UserContext.defaultUser()` 外零硬编码

**权限插槽**

- [ ] 主操作入口（增 / 改 / 删 / 审批）都有 `can()` 判断
- [ ] `PermissionService.can()` 存在且返回 true

**认证预留**

- [ ] `UserContext` 存在，业务代码统一从 `.current()` 取身份
- [ ] `AuthFilter` 已注册（`@WebFilter("/*")`）且 finally 里 `clear()`
- [ ] `AuthConfig.AUTH_MODE = "none"`

**文案主题**

- [ ] 界面文字全部走 `t()`
- [ ] 颜色全部走 CSS 变量，无 `#xxxxxx` 硬编码
- [ ] 菜单由 `menu.js` 配置数组渲染

---

## 12. 后期改造地图（需求 → 改哪里）

| 用户后期提出 | 要改的地方 | 成本 |
|---|---|---|
| "要加登录" | `AuthConfig` + `AuthFilter` + `LoginServlet` + `users` 表 | 低 |
| "店员不能看成本价" | `PermissionService.can()` + DAO 取列控制 | 低 |
| "只看自己门店的数据" | `DataScope.clause()` 加 `org_id` 条件 | 低 |
| "要英文版" | 新增 `en.js` / `messages_en.properties` | 低 |
| "要深色主题" | 新增一组 CSS 变量 | 极低 |
| "A 角色能改、B 只能看" | `PermissionService` 实现（调用点已存在） | 中 |
| "拆成多公司共用" | `org_id` 填充 + `DataScope` 全局过滤（接缝已留） | 中 |

**这张表说明**：因为四个接缝都在，所有后期改造都是"改一处"，不是"改一遍"。

---

## 13. 与开发计划的关系

AI 编码生成的"开发计划"，**必须把接缝步骤列为独立一步**，不可跳过、不可合并，固定放在最前：

```
第 1 步：搭好架子（四尾字段 + DataScope + PermissionService + UserContext + AuthFilter + api.js）
第 2 步：建数据表（含四尾与索引）
第 3~N 步：业务流程功能
```

**理由**：接缝成本只有 10%，漏掉后改造要 10 倍代价。
它不是"优化项"，是"地基项"，必须显式出现在用户可见的计划里（对应 7.2 的 A 类）。

---

## 14. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-11-05 | 新增第 7~13 章：预留式 MVP（三类改动分级 / 四个接缝 / 认证预留 / AI 编码铁律 / 自检清单 / 改造地图） |
| 2026-11-05 | 三文档去重：AGENTS 核心约定与反模式改引用；第 3/4/6 章写法类条款改引用；第 5 章 Redis 细则整章归并至 CODING-STANDARDS 第 3 节 |
| 2026-11-05 | 全文一致性整合：补 `model` 层与 Filter 分层位置；补 `BizException` 约定；`can()` 与 `UserContext` 边界澄清；标注身份字面量唯一例外；统一错误文案走 key；补登录页白名单与 YAGNI 边界 |
