fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=res/resource.h");

    // 构建脚本不受「库/UI 路径禁用 unwrap/panic」约束；这里显式报错，避免路径退化成 "\app.res"
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(dir) => dir,
        Err(e) => panic!("构建脚本未拿到 OUT_DIR 环境变量（应由 cargo 注入）：{e}"),
    };
    let res_out = format!("{out_dir}\\app.res");
    // 任何构建形态都编译一次 .rc，资源脚本写错能第一时间暴露
    let status = std::process::Command::new("rc")
        .args(["/nologo", "/fo", &res_out, "res/app.rc"])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("rc.exe 编译资源失败，退出码 {:?}", s.code()),
        Err(e) => panic!("找不到 rc.exe（需要 MSVC Build Tools，或用 build.ps1 导入 vcvars 环境）：{e}"),
    }

    // 只有发布构建才把资源链接进 exe。
    // cargo 不会告诉构建脚本当前是不是 `cargo test`（实测 CARGO_CFG_TEST、CARGO_PROFILE_*
    // 在 build script 环境里都不存在），因此按 OUT_DIR 的路径形态判定 release。
    // 原因：debug/test 产物一旦带上 requireAdministrator，非提升的 shell 里
    // cargo test 连测试可执行文件都无法启动（os error 740「需要提升」），
    // 阶段1 各任务的单元测试就全都跑不起来；UI 验收一律用 --release 产物。
    let is_release = std::path::Path::new(&out_dir)
        .components()
        .any(|part| part.as_os_str() == "release");
    if is_release {
        // link.exe 可直接接收 .res 作为输入；用绝对路径避免工作目录歧义
        println!("cargo:rustc-link-arg-bin=nictool={res_out}");
    }
}
