use std::{env, error::Error, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=resources/app.rc");
    println!("cargo:rerun-if-changed=resources/icons/iCapture.ico");
    for name in [
        "RC",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "ProgramFiles(x86)",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let output =
        PathBuf::from(env::var_os("OUT_DIR").ok_or("缺少 Cargo OUT_DIR")?).join("icapture.res");
    let compiler = resource_compiler()?;
    let status = Command::new(compiler)
        .args(["/nologo", "/fo"])
        .arg(&output)
        .arg("resources/app.rc")
        .status()?;
    if !status.success() {
        return Err("图标资源编译失败，请检查 Windows SDK 和 resources/app.rc".into());
    }
    println!("cargo:rustc-link-arg={}", output.display());
    Ok(())
}

fn resource_compiler() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = env::var_os("RC") {
        return Ok(path.into());
    }
    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths) {
            let path = directory.join("rc.exe");
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ProgramFiles(x86)").map(|root| PathBuf::from(root).join("Windows Kits/10"))
        })
        .ok_or("找不到 Windows SDK，请设置 RC 为 rc.exe 路径")?;
    // rc.exe is a host tool; the generated resource works for the target architecture.
    let host = env::var("HOST")?;
    let architecture = if host.starts_with("aarch64") {
        "arm64"
    } else if host.starts_with("x86_64") {
        "x64"
    } else {
        "x86"
    };
    let bin = sdk.join("bin");
    if let Some(version) = env::var_os("WindowsSDKVersion") {
        let path = bin.join(version).join(architecture).join("rc.exe");
        if path.is_file() {
            return Ok(path);
        }
    }
    let mut versions = std::fs::read_dir(&bin)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    versions.sort();
    versions.reverse();
    versions
        .into_iter()
        .map(|path| path.join(architecture).join("rc.exe"))
        .find(|path| path.is_file())
        .ok_or_else(|| "找不到 rc.exe，请安装 Windows SDK 或设置 RC 路径".into())
}
