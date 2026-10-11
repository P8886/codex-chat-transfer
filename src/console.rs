mod commands;
#[cfg(windows)]
mod desktop;
#[cfg(windows)]
mod single_instance;
mod web;

fn main() {
    if let Err(error) = commands::run(false) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
