pub mod transpile;
pub mod ffi;
pub mod hooks;

#[cfg(feature = "qt")]
extern "C" {
    fn run_fdf_app() -> i32;
}

#[cfg(feature = "qt")]
pub fn run_app() -> ! {
    let exit_code = unsafe { run_fdf_app() };
    std::process::exit(exit_code);
}

#[cfg(not(feature = "qt"))]
pub fn run_app() -> ! {
    eprintln!("FDF app runtime requires Qt. Build with --features qt or use the bridge build command instead.");
    std::process::exit(1);
}
