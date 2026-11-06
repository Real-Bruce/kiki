#![windows_subsystem = "windows"]

mod core;
mod platform;

fn main() -> std::process::ExitCode {
    if let Err(error) = platform::windows::run() {
        platform::windows::show_error(&error.to_string());
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
