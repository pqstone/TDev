fn main() {
    // 前端资源在编译期嵌入二进制：必须声明追踪，否则改了 dist/ 不会重新打包
    println!("cargo:rerun-if-changed=../dist");
    println!("cargo:rerun-if-changed=../dist/index.html");
    tauri_build::build()
}
