package com.example.dao;

import com.example.model.Order;
import com.example.util.JdbcUtil;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.util.ArrayList;
import java.util.List;

/**
 * DAO 模板：纯 JDBC、参数化查询、非贪婪取列、try-with-resources。
 * 新增 DAO 时复制本文件，仅替换表名/字段/业务方法。
 */
public class OrderDao {

    /** 查询单条：只取当前功能需要的列，禁止 SELECT * */
    public Order findById(long id) {
        // 非贪婪取列：只 select 用到的字段
        String sql = "SELECT id, order_no, user_id, amount, status FROM orders WHERE id = ?";
        try (Connection conn = JdbcUtil.getConnection();
             PreparedStatement ps = conn.prepareStatement(sql)) {
            ps.setLong(1, id);
            try (ResultSet rs = ps.executeQuery()) {
                if (rs.next()) {
                    Order o = new Order();
                    o.setId(rs.getLong("id"));
                    o.setOrderNo(rs.getString("order_no"));
                    o.setUserId(rs.getLong("user_id"));
                    o.setAmount(rs.getBigDecimal("amount"));
                    o.setStatus(rs.getInt("status"));
                    return o;
                }
            }
        } catch (Exception e) {
            throw new RuntimeException("查询订单失败", e);
        }
        return null;
    }

    /** 分页查询：LIMIT 参数化，禁止无 limit 全表扫描 */
    public List<Order> findByUserId(long userId, int page, int size) {
        String sql = "SELECT id, order_no, amount, status FROM orders WHERE user_id = ? ORDER BY id DESC LIMIT ?, ?";
        List<Order> list = new ArrayList<>();
        try (Connection conn = JdbcUtil.getConnection();
             PreparedStatement ps = conn.prepareStatement(sql)) {
            ps.setLong(1, userId);
            ps.setInt(2, (page - 1) * size);
            ps.setInt(3, size);
            try (ResultSet rs = ps.executeQuery()) {
                while (rs.next()) {
                    Order o = new Order();
                    o.setId(rs.getLong("id"));
                    o.setOrderNo(rs.getString("order_no"));
                    o.setAmount(rs.getBigDecimal("amount"));
                    o.setStatus(rs.getInt("status"));
                    list.add(o);
                }
            }
        } catch (Exception e) {
            throw new RuntimeException("分页查询订单失败", e);
        }
        return list;
    }

    /** 插入：参数化，返回受影响行数并校验 */
    public int insert(Order o) {
        String sql = "INSERT INTO orders (order_no, user_id, amount, status) VALUES (?, ?, ?, ?)";
        try (Connection conn = JdbcUtil.getConnection();
             PreparedStatement ps = conn.prepareStatement(sql)) {
            ps.setString(1, o.getOrderNo());
            ps.setLong(2, o.getUserId());
            ps.setBigDecimal(3, o.getAmount());
            ps.setInt(4, o.getStatus());
            int rows = ps.executeUpdate();
            if (rows == 0) {
                throw new RuntimeException("插入订单失败，受影响行数为 0");
            }
            return rows;
        } catch (Exception e) {
            throw new RuntimeException("插入订单失败", e);
        }
    }
}
