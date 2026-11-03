package com.example.controller;

import com.example.service.OrderService;
import com.example.util.JsonUtil;

import javax.servlet.annotation.WebServlet;
import javax.servlet.http.HttpServlet;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.io.IOException;

/**
 * Servlet 模板：只做「解析参数 → 调 service → 统一 JSON 响应」。
 * 禁止在本层写 SQL、写业务逻辑。
 */
@WebServlet("/api/order/list")
public class OrderListServlet extends HttpServlet {

    private final OrderService orderService = new OrderService();

    @Override
    protected void doGet(HttpServletRequest req, HttpServletResponse resp) throws IOException {
        resp.setContentType("application/json;charset=UTF-8");

        // 1. 解析参数（带默认值，禁止裸用可能为 null 的参数）
        long userId = parseLong(req.getParameter("userId"), 0);
        int page = parseInt(req.getParameter("page"), 1);
        int size = parseInt(req.getParameter("size"), 20);

        if (userId <= 0) {
            JsonUtil.writeJson(resp, 400, "userId 必填", null);
            return;
        }

        // 2. 调 service（业务与事务在 service 层）
        Object data = orderService.pageQueryByUser(userId, page, size);

        // 3. 统一 JSON 响应
        JsonUtil.writeJson(resp, 0, "ok", data);
    }

    // 参数解析工具（放 util 层更佳，此处为模板内联示意）
    private static long parseLong(String v, long def) {
        try { return v == null ? def : Long.parseLong(v.trim()); } catch (NumberFormatException e) { return def; }
    }
    private static int parseInt(String v, int def) {
        try { return v == null ? def : Integer.parseInt(v.trim()); } catch (NumberFormatException e) { return def; }
    }
}
