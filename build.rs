use std::{env, error::Error, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=VERSION");
    println!("cargo:rerun-if-changed=resources/app.rc");
    println!("cargo:rerun-if-changed=resources/app.manifest");
    println!("cargo:rerun-if-changed=resources/icons/kiki.ico");
    for name in [
        "RC",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "ProgramFiles(x86)",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let version_file = fs::read_to_string("VERSION")?;
    let version = version_file.trim();
    let cargo_version = env::var("CARGO_PKG_VERSION")?;
    if version != cargo_version {
        return Err("VERSION 与 Cargo.toml 不一致，请运行 scripts/Sync-Version.ps1".into());
    }
    let major = env::var("CARGO_PKG_VERSION_MAJOR")?.parse::<u16>()?;
    let minor = env::var("CARGO_PKG_VERSION_MINOR")?.parse::<u16>()?;
    let patch = env::var("CARGO_PKG_VERSION_PATCH")?.parse::<u16>()?;
    if version != format!("{major}.{minor}.{patch}") {
        return Err("VERSION 必须是 X.Y.Z 格式，各段取值为 0 至 65535".into());
    }
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("缺少 Cargo OUT_DIR")?);
    let manifest = out_dir.join("kiki.manifest");
    let template = fs::read_to_string("resources/app.manifest")?;
    fs::write(&manifest, template.replace("@KIKI_VERSION@", version))?;
    let output = out_dir.join("kiki.res");
    let compiler = resource_compiler()?;
    let status = Command::new(compiler)
        .args(["/nologo", "/fo"])
        .arg(&output)
        .arg(format!("/dKIKI_VERSION_NUMBERS={major},{minor},{patch},0"))
        .arg(format!("/dKIKI_VERSION_STRING=\"{version}\""))
        .arg("resources/app.rc")
        .status()?;
    if !status.success() {
        return Err("Windows 资源编译失败，请检查 Windows SDK 和 resources/app.rc".into());
    }
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
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
