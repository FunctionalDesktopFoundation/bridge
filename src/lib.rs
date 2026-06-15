extern "C" {
    fn run_trash_app() -> i32;
}

pub fn run_app() -> ! {
    let exit_code = unsafe { run_trash_app() };
    std::process::exit(exit_code);
}
