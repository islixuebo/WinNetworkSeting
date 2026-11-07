fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=res/resource.h");

    // 构建脚本不受「库/UI 路径禁用 unwrap/panic」约束；这里显式报错，避免路径退化成 "\app.res"
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(dir) => dir,
        Err(e) => panic!("构建脚本未拿到 OUT_DIR 环境变量（应由 cargo 注入）：{e}"),
    };
    // 用 PathBuf::join 而不是手拼 "\\"，分隔符形态交给标准库决定
    let res_out = std::path::PathBuf::from(&out_dir)
        .join("app.res")
        .to_string_lossy()
        .into_owned();
    // 任何构建形态都编译一次 .rc，资源脚本写错能第一时间暴露
    let status = std::process::Command::new("rc")
        .args(["/nologo", "/fo", &res_out, "res/app.rc"])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("rc.exe 编译资源失败，退出码 {:?}", s.code()),
        Err(e) => {
            panic!("找不到 rc.exe（需要 MSVC Build Tools，或用 build.ps1 导入 vcvars 环境）：{e}")
        }
    }

    // 只有发布构建才把资源链接进 exe（门控理由见下）。
    // 判定依据是 cargo 为 build script 专门设置的 `PROFILE` 环境变量
    // （release 构建为 "release"，其余形态为 "debug"）；
    // 上一轮那条「cargo 不向 build script 透露 profile，只能按 OUT_DIR 路径嗅探」的注释是错的——
    // 当时的探针只 grep 了 `CARGO_*` 前缀，漏掉不带前缀的 PROFILE / DEBUG / OPT_LEVEL。
    // 门控本身仍然必要：debug/test 产物一旦带上 requireAdministrator，
    // 非提升的 shell 里 cargo test 连测试可执行文件都无法启动（os error 740「需要提升」），
    // 阶段1 各任务的单元测试就全都跑不起来。
    let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
    if is_release {
        // link.exe 可直接接收 .res 作为输入；用绝对路径避免工作目录歧义
        println!("cargo:rustc-link-arg-bin=nictool={res_out}");
    } else {
        // 护栏：debug 形态刻意不链接资源，避免下一次「窗口不出来 / 对话框创建失败」被当成代码 bug
        println!(
            "cargo:warning=debug 形态不链接 app.res（否则 cargo test 的测试壳会因 requireAdministrator \
             被拒绝启动，os error 740）：UI 验收请用 `cargo build --release --target aarch64-pc-windows-msvc` \
             的产物；跑单元测试请用不带 --release 的 `cargo test`，切勿用 `cargo test --release`"
        );
    }
}
