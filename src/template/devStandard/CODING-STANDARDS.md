# CODING-STANDARDS.md —— 代码规范与自检清单

> 定义"整洁、低冗余、规范"的具体标准。所有 Agent 交付前必须逐项通过末尾自检清单。
> **边界**：本文件是**代码写法（how）的唯一来源**；分层职责、四个接缝、认证预留等架构决策（why）见 `ARCHITECTURE.md`；目录结构与开工流程见 `AGENTS.md`。
> 程度词：`必须`/`禁止`为硬约束，违反即返工；`推荐`为最佳实践。

---

## 1. Java 代码规范

### 1.1 命名
- 类名：`UpperCamelCase`（`OrderService`）；方法/变量：`lowerCamelCase`（`getOrderById`）；常量：`UPPER_SNAKE`。
- 布尔方法/字段用 `isXxx` / `hasXxx`。
- 包名：全小写，按层分包（`controller/service/dao/model/util/redis/ws`），禁止按功能碎片建包。

### 1.2 方法与类
- 方法**必须 ≤ 40 行**，超过即拆分（抽取私有方法/工具类）。
- 类职责单一：一个类只做一件事；禁止"上帝类"。
- 重复代码（≥3 处相似）**必须**抽公共方法，禁止复制粘贴。
- 公共 API 必须写 Javadoc；复杂业务逻辑必须写注释说明"为什么"。

### 1.3 异常处理
- 禁止空捕获：`catch (Exception e) {}`。
- 业务可预期错误抛自定义业务异常（`BizException`），由 controller 统一转 JSON。
- 技术异常（SQL、连接）记录日志并向上抛出，由统一处理点兜底。
- `finally`/`try-with-resources` 中必须释放资源，禁止泄漏。

### 1.4 日志
- 使用统一日志（slf4j + logback），**禁止** `System.out.println`。
- 日志分级：错误场景 `error`（含堆栈），业务关键节点 `info`，调试 `debug`。
- 禁止打印敏感信息（密码、token、身份证号）。

### 1.5 资源与连接
- JDBC：连接、Statement、ResultSet **必须** `try-with-resources` 或 finally 释放。
- 数据库连接走连接池，禁止每次 new Connection。
- 线程：禁止裸 `new Thread` 执行任务，业务异步走线程池并规范命名。

---

## 2. SQL 规范（纯 JDBC，非贪婪取列）

### 2.1 强制项
- **必须**用 `PreparedStatement` 占位符（`?`）传参，**禁止**字符串拼接 SQL（防注入）。
- **必须非贪婪取列**：只 select 当前功能用到的列，**禁止 `SELECT *`**。
  ```java
  // ✅ 只取需要的列
  String sql = "SELECT id, order_no, amount FROM orders WHERE user_id = ?";
  // ❌ SELECT * FROM orders WHERE user_id = ?
  ```
- 结果集**按列名精确取值**，取不到的列不引用。
- 写操作（insert/update/delete）返回受影响行数并校验（`==0` 视为失败或提示）。
- 查询必须带条件过滤；无条件的全表查询**禁止**。

### 2.2 分页
- 列表接口必须 `LIMIT offset, size`（或 `LIMIT ?,?`），参数化传值。
- 大数据量禁止大 offset 深翻页，改用键集/游标分页。

### 2.3 事务
- 涉及多表/多步写操作，在 service 层统一 `begin → 操作 → commit/rollback`。
- 事务尽量短：只包住必要的写操作，不把读操作和长循环放进事务。

---

## 3. Redis 使用规范

1. key 命名：`业务域:对象:标识`（如 `user:info:1001`），全小写。
2. 每次写入**必须**设置 TTL（`SET key val EX n`），禁止无过期 key。
3. 读流程：先查缓存 → 未命中查库 → 回填缓存（设置 TTL）。
4. 写流程：**先更新数据库，再删除缓存**（Cache Aside）；禁止先删缓存后写库（不一致窗口大）。
5. 禁止大 key：不缓存整表/大列表；列表类缓存按页或只缓存热点。
6. Redis 客户端统一走 `redis/` 封装层，禁止各业务类直接 new 连接。
7. key 变更/删除必须走统一封装方法，禁止散落 `DEL` 语句。

---

## 4. 前端规范（纯 HTML + 原生 JS）

1. **零依赖**：不引任何第三方库/框架/CDN 脚本。
2. 命名：JS 变量/函数 `lowerCamelCase`，常量 `UPPER_SNAKE`，CSS 类 `kebab-case`。
3. XHR 封装：统一一个 `api.js` 封装 `XMLHttpRequest`（GET/POST、超时、错误处理），页面不裸写 XHR。
4. WebSocket：统一封装连接、**断线自动重连（指数退避）**、消息路由；页面不裸 new WebSocket 分散管理。
5. DOM 操作集中在 `DOMContentLoaded` 后；禁止内联 `onclick` 大段逻辑（用事件委托/监听）。
6. 字符串拼接 HTML 时**必须转义用户输入**（防 XSS）。
7. 全局变量**禁止**随意扩散，业务状态收敛到模块级封装；跨页面通信用 `localStorage`/自定义事件。
8. 代码复用：公共函数进 `util.js`，禁止页面间复制粘贴。

### 4.9 界面布局与视觉秩序（新增或修改任何界面必读）

**动手前先读 `css/app.css` 和 `js/views/` 下同类页面。已有类直接复用，禁止重新定义。**
（现存 `.form-grid` / `.field` / `.field-label` / `.btn-danger` / `.toolbar` / `.table-empty` 等 **10 个类都被重复定义过两遍**，后定义的覆盖先定义的 —— 这是页面越改越乱的结构性原因。）

#### 一、节约版面（一屏能看多少信息）

1. 表单按字段长度分栏，一行排满，不许一个短字段独占一行：
   - 短字段（姓名 / 编号 / 数量 / 日期 / 状态）：一行 **3~4 个**
   - 中等字段（标题 / 分类 / 金额 / 联系人）：一行 **2 个**
   - 长文本（描述 / 备注 / 要求）：**整行独占**，`textarea` 高 3~4 行
   - 图片、明细子表：整行独占
2. 列表页首屏至少能看到 **10 行数据**，默认一页 20 行；不要一页 5 行还留大片空白。
3. 详情页按业务分组，每组一个 `.form-section` + `.form-section-title`，组内两列排布；不要一长条往下堆。
4. 弹窗宽度按内容三档：**480 / 640 / 800**，禁止全屏弹窗。表单字段超过 8 个必须分步，或改成抽屉。
5. 同一屏内边距统一，不许这块 12px 那块 30px。

#### 二、整齐秩序（一眼看出是同一套系统）

1. **只用 8px 栅格**：margin / padding / gap 只能取 `--space-xs`(4) `--space-sm`(8) `--space-md`(12) `--space-lg`(16) `--space-xl`(24) `--space-xxl`(32)。**禁止 5px / 7px / 13px / 18px 这类随手值。**
2. **标签统一**：同一表单内所有 `.field-label` 用同一字号（`--font-caption`）与对齐方式；横排表单 label 固定宽度 **96px 右对齐**。
3. **控件高度统一**：输入框 / 下拉框 / 按钮统一 `height: var(--control-height)`（32px），强调场景用 `--control-height-lg`（40px）。一行内不许高低不齐。
4. **按钮位置固定**：
   - 表单：主操作（保存 / 提交）在**右下**，取消在它左边
   - 列表页：主操作（新增）在**表格上方右侧**
   - 删除类（`.btn-danger`）**单独放**，不挨着主操作
5. **一屏只有一个主按钮**（`.btn-primary` 实心），其余一律 `.btn` 描边或 `.btn-link`。
6. **表格对齐**：文本列左对齐；数字 / 金额右对齐并加 `font-variant-numeric: tabular-nums`；状态列居中；列宽按内容分配，不要所有列等宽。
7. 圆角只用 `--radius-control`(4) / `--radius-card`(8) / `--radius-pill`(999)，不许自造数值。
8. **菜单层级不超过 2 级**：左侧主导航、右侧功能页内的功能菜单与分组，一律只允许「一级 + 二级」两层。
   需要第三层时，重新归并到同级、或改成页内 tab / 分区折叠，**不许做三级下拉、三级导航**。
   理由：层级一深，用户找不到功能，面包屑和返回路径也会失控；2 级是本项目的硬上限。

#### 三、审美（留白、层级、克制）

1. **留白优先**：宁可少放一个字段，也不要挤成一团。组间距 `--space-xl`(24)，字段间距 `--space-md`(12)。
2. **层级靠字号 + 字重 + 灰度**，不靠加粗加色：
   - 页标题 `--font-title`(24/600)、区块标题 `--font-subtitle`(20/600)
   - 正文 `--font-body`(14/400)、辅助说明 `--font-caption`(12/400)
   - 文字色三档：`--color-text-primary` / `--color-text-secondary` / `--color-text-tertiary`
3. **色彩克制**：`--color-accent` 只用于主操作、选中态、关键数字；正文一律中性色；**禁止大面积铺主色**。
4. **三种状态必须画全**：空状态（一句说明 + 一个引导按钮，参考 `.table-empty`）、加载态、错误态（原因 + 重试按钮）。**不许白屏**。
5. 中文字号不小于 12px，正文行高 1.5。

#### 四、标准骨架（照抄，不要自己发明）

```html
<div class="form-section">
  <div class="form-section-title">客户信息</div>
  <div class="grid grid-3">
    <label class="field">
      <span class="field-label">客户姓名</span>
      <input type="text" name="customerName" />
    </label>
    <label class="field">
      <span class="field-label">联系方式</span>
      <input type="text" name="contact" />
    </label>
    <label class="field">
      <span class="field-label">期望报价</span>
      <input type="number" name="expectPrice" />
    </label>
    <label class="field" style="grid-column:1 / -1">
      <span class="field-label">产品描述</span>
      <textarea name="desc" rows="4"></textarea>
    </label>
  </div>
  <div class="form-actions">
    <button class="btn" type="button">取消</button>
    <button class="btn btn-primary" type="submit">保存</button>
  </div>
</div>
```

配套样式（`app.css` 里若没有 `.form-actions`，**只加这一次**，之后所有页面复用）：

```css
.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-sm);
  padding-top: var(--space-md);
  grid-column: 1 / -1;
}
```

列表页骨架：

```html
<div class="toolbar" style="justify-content:flex-end">
  <button class="btn btn-primary" id="btnAdd">新增</button>
</div>
<div class="table-scroll">
  <table class="table">
    <thead><tr><th>编号</th><th>客户</th><th style="text-align:right">金额</th><th style="text-align:center">状态</th></tr></thead>
    <tbody id="rows"></tbody>
  </table>
</div>
```

#### 五、交付前 UI 自检

- [ ] 一行排满了？没有半屏空白？
- [ ] 所有间距都是 4 的倍数，没有 5 / 7 / 13 / 18？
- [ ] 同一表单 label 字号一致、输入框高度一致？
- [ ] 一屏只有一个实心主按钮，位置在右下 / 右上？
- [ ] 空 / 加载 / 错误三态齐全？
- [ ] 颜色、字号、圆角、间距全部走 CSS 变量，没有裸值？
- [ ] 没有重复定义 `app.css` 里已有的类？
- [ ] 数字列右对齐、文本列左对齐？
- [ ] 菜单层级没超过 2 级（没有三级下拉 / 三级导航）？


---

## 5. 代码整洁与低冗余（总原则）

- **DRY**：同一逻辑只出现一次；重复 ≥3 处必须抽取。
- **YAGNI**：只写当前需要的代码，禁止为"未来可能"堆砌无用抽象。（**例外**：ARCHITECTURE.md 第 8 章的"接缝"是零逻辑调用点占位，不是抽象层，判定标准见该文 7.3）
- 禁止无用 import、无用变量、注释掉的死代码（直接删除）。
- 禁止深层嵌套（>3 层）——用提前 return / 卫语句改写。
- 每个文件职责单一；文件名与内容一致（`OrderService.java` 里只放订单相关）。

---

## 6. 交付自检清单（Agent 交付前逐项核对）

**架构与落位**
- [ ] 代码落在 ARCHITECTURE.md 规定的职责目录，无跨层调用。
- [ ] 未引入任何未确认的新依赖/框架。

**Java**
- [ ] 方法 ≤ 40 行；类职责单一；无重复代码（≥3 处已抽取）。
- [ ] 无空捕获、无 `System.out`、无敏感信息日志。
- [ ] 连接/Statement/ResultSet 已用 try-with-resources 释放。
- [ ] 事务在 service 层正确开启/提交/回滚。

**SQL**
- [ ] 全部使用 PreparedStatement 参数化，无字符串拼接 SQL。
- [ ] 无 `SELECT *`；只查当前功能需要的列，结果集按列名取值。
- [ ] 列表接口带分页（LIMIT 参数化）。
- [ ] 查询条件列有索引；建表脚本已加入 `sql/`。

**Redis**
- [ ] 所有 key 有 TTL；key 命名符合 `业务域:对象:标识`。
- [ ] 缓存一致性按"先更库、后删缓存"；无大 key、无整表缓存。
- [ ] Redis 调用统一走 `redis/` 封装层。

**前端**
- [ ] 零第三方依赖；XHR/WebSocket 走统一封装。
- [ ] WebSocket 有断线重连；用户输入已转义（防 XSS）。
- [ ] 无裸内联大段逻辑、无页面间复制粘贴。

**通用**
- [ ] 接口响应符合统一 JSON 格式。
- [ ] 无死代码、无用 import、注释掉的代码。
- [ ] 新代码与现有代码风格一致，命名规范。
