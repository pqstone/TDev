// ============================================================
// TDev 前端源码（模块化参考）
// 说明：本骨架为纯静态前端，运行时成品直接维护在 dist/ 下。
//       如需引入打包工具（Vite 等），本目录为源码入口，构建产物
//       输出到 dist/（tauri.conf.json 的 frontendDist 指向 ../dist）。
// ============================================================

// 统一 IPC 封装：所有对 Rust 后端的调用一律走此函数，便于统一错误处理
export async function invokeCmd(cmd, args) {
  if (!window.__TAURI__ || !window.__TAURI__.core) {
    throw new Error("未运行于 Tauri 桌面环境");
  }
  return window.__TAURI__.core.invoke(cmd, args || {});
}
