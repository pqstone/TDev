// Windows 下以 GUI 子系统运行：Release 构建不弹控制台窗口；Debug 构建保留控制台以便查看 println 日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tdev_lib::run()
}
