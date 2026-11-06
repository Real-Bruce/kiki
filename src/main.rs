#![windows_subsystem = "windows"]

mod core;
mod platform;

fn main() {
    if let Err(error) = platform::windows::run() {
        platform::windows::show_error(&error.to_string());
    }
}
