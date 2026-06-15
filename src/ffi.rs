use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::Mutex;

static REGISTRY: Mutex<Option<HashMap<String, Box<dyn Fn(Vec<String>) -> String + Send>>>> = Mutex::new(None);

pub fn register(name: &str, f: Box<dyn Fn(Vec<String>) -> String + Send>) {
    let mut registry = REGISTRY.lock().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry.as_mut().unwrap().insert(name.to_string(), f);
}

pub fn call(name: &str, args: Vec<String>) -> String {
    let registry = REGISTRY.lock().unwrap();
    match registry.as_ref().and_then(|r| r.get(name)) {
        Some(f) => f(args),
        None => format!("error: unknown function '{}'", name),
    }
}

pub fn is_registered(name: &str) -> bool {
    let registry = REGISTRY.lock().unwrap();
    registry.as_ref().map_or(false, |r| r.contains_key(name))
}

pub fn list_functions() -> Vec<String> {
    let registry = REGISTRY.lock().unwrap();
    registry.as_ref().map_or(vec![], |r| r.keys().cloned().collect())
}

pub fn load_plugins(dir: &Path) {
    if !dir.exists() || !dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "so") {
            continue;
        }
        match load_plugin(&path) {
            Ok(count) => {
                if count > 0 {
                    eprintln!("[ffi] loaded {} functions from {}", count, path.display());
                }
            }
            Err(e) => {
                eprintln!("[ffi] failed to load {}: {}", path.display(), e);
            }
        }
    }
}

fn load_plugin(path: &Path) -> Result<usize, String> {
    let lib = unsafe {
        libloading::Library::new(path).map_err(|e| format!("libloading: {}", e))?
    };
    let version: libloading::Symbol<extern "C" fn() -> i32> = unsafe {
        lib.get(b"ffi_plugin_version").map_err(|_| "not an FDF plugin (no ffi_plugin_version)".to_string())?
    };
    if version() != 1 {
        return Err("unsupported plugin version".to_string());
    }
    let count = std::sync::atomic::AtomicUsize::new(0);
    let count_ref = &count;

    extern "C" fn c_register(name: *const std::os::raw::c_char, fn_ptr: extern "C" fn(*const std::os::raw::c_char) -> *mut std::os::raw::c_char, ctx: *mut std::os::raw::c_void) {
        let c_name = unsafe { CStr::from_ptr(name) };
        let name_str = c_name.to_string_lossy().to_string();
        let count = unsafe { &*(ctx as *const std::sync::atomic::AtomicUsize) };
        register(&name_str, Box::new(move |args| {
            let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "[]".to_string());
            let c_args = CString::new(args_json).unwrap_or_default();
            let result_ptr = fn_ptr(c_args.as_ptr());
            let result = if result_ptr.is_null() {
                String::new()
            } else {
                let s = unsafe { CStr::from_ptr(result_ptr) }.to_string_lossy().to_string();
                unsafe { let _ = CString::from_raw(result_ptr); }
                s
            };
            result
        }));
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    let init: libloading::Symbol<extern "C" fn(extern "C" fn(*const std::os::raw::c_char, extern "C" fn(*const std::os::raw::c_char) -> *mut std::os::raw::c_char, *mut std::os::raw::c_void), *mut std::os::raw::c_void)> = unsafe {
        lib.get(b"ffi_plugin_init").map_err(|_| "plugin missing ffi_plugin_init".to_string())?
    };
    init(c_register, count_ref as *const _ as *mut std::os::raw::c_void);

    std::mem::forget(lib);

    Ok(count.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn register_from_json(json: &str) -> Result<usize, String> {
    #[derive(serde::Deserialize)]
    struct Def {
        fns: Vec<FnDef>,
    }
    #[derive(serde::Deserialize)]
    struct FnDef {
        name: String,
        code: Option<String>,
    }
    let def: Def = serde_json::from_str(json).map_err(|e| format!("invalid FFI JSON: {}", e))?;
    let mut count = 0;
    for f in &def.fns {
        let name = f.name.clone();
        let code = f.code.clone().unwrap_or_default();
        register(&name, Box::new(move |args| {
            let _ = (&args, &code);
            format!("ok")
        }));
        count += 1;
    }
    Ok(count)
}

#[no_mangle]
pub extern "C" fn rust_ffi_has(name: *const std::os::raw::c_char) -> i32 {
    let c_str = unsafe { CStr::from_ptr(name) };
    let name_str = c_str.to_string_lossy();
    if is_registered(&name_str) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn rust_ffi_call(name: *const std::os::raw::c_char, args_json: *const std::os::raw::c_char) -> *mut std::os::raw::c_char {
    let c_name = unsafe { CStr::from_ptr(name) };
    let c_args = unsafe { CStr::from_ptr(args_json) };
    let name_str = c_name.to_string_lossy();
    let args_str = c_args.to_string_lossy();
    let args: Vec<String> = if args_str.starts_with('[') {
        serde_json::from_str(&args_str).unwrap_or_default()
    } else if args_str.is_empty() {
        vec![]
    } else {
        args_str.split(',').map(|s| s.trim().to_string()).collect()
    };
    let result = call(&name_str, args);
    CString::new(result).unwrap_or_default().into_raw()
}

#[no_mangle]
pub extern "C" fn rust_ffi_free_string(s: *mut std::os::raw::c_char) {
    if !s.is_null() {
        unsafe { let _ = CString::from_raw(s); }
    }
}

#[no_mangle]
pub extern "C" fn rust_ffi_list() -> *mut std::os::raw::c_char {
    let funcs = list_functions();
    let json = serde_json::to_string(&funcs).unwrap_or_else(|_| "[]".to_string());
    CString::new(json).unwrap_or_default().into_raw()
}
