# scaffold/ —— 代码生成模板

> **约定：新增任何模块/接口，必须基于本目录模板生成，禁止从零画结构。**
> 复制对应模板 → 改名 → 填入业务逻辑，保持结构与写法不变。

## 文件清单

| 模板 | 用途 | 复制到 |
|---|---|---|
| `DaoTemplate.java` | 纯 JDBC 数据访问（非贪婪取列示例） | `src/main/java/.../dao/` |
| `ServletTemplate.java` | Servlet 接口（统一 JSON 响应） | `src/main/java/.../controller/` |
| `WebPageTemplate.html` | 前端页面（XHR + WebSocket 重连封装） | `src/main/webapp/` |

## 生成步骤

1. 复制模板到对应目录并改名（如 `OrderDao.java`）。
2. 按 AGENTS.md 目录结构确认落位正确。
3. 按 CODING-STANDARDS.md 填入业务逻辑（SQL 参数化、非贪婪取列、TTL 等）。
4. 交付前跑 CODING-STANDARDS.md 末尾自检清单。
