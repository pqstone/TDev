// 部署引擎真机探针
//   cargo run --release --bin probe                      → 完整管道
//   cargo run --release --bin probe -- build <dir> <cmd> → 只跑构建
//   cargo run --release --bin probe -- ssh <cmd>         → 在服务器上执行一条命令
fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "build" {
        match tdev_lib::deploy::probe_build_once(&args[2], &args[3]) {
            Ok(m) => println!("RESULT OK: {}", m),
            Err(e) => { println!("RESULT FAIL: {}", e); std::process::exit(1); }
        }
        return;
    }
    if args.len() >= 2 && args[1] == "prepare" {
        match rt.block_on(tdev_lib::deploy::probe_agent_prepare()) {
            Ok(o) => println!("RESULT OK:
{}", o),
            Err(e) => { println!("RESULT FAIL: {}", e); std::process::exit(1); }
        }
        return;
    }
    if args.len() >= 3 && args[1] == "ssh" {
        match rt.block_on(tdev_lib::deploy::probe_ssh_exec(&args[2])) {
            Ok(o) => println!("{}", o),
            Err(e) => { println!("RESULT FAIL: {}", e); std::process::exit(1); }
        }
        return;
    }
    match rt.block_on(tdev_lib::deploy::probe_pipeline_standalone()) {
        Ok(msg) => println!("RESULT OK: {}", msg),
        Err(e) => { println!("RESULT FAIL: {}", e); std::process::exit(1); }
    }
}
