use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

type Handle = *mut c_void;

pub const BUILD_DEFAULT: c_int = 15;
pub const MATCH_ALL_WITH_NORMALIZING: c_int = 0x3f | (1 << 23) | (1 << 16);
pub const TYPO_BASIC: c_int = 1;

#[cfg(windows)]
pub const LIB_FILE: &str = "kiwi.dll";
#[cfg(target_os = "macos")]
pub const LIB_FILE: &str = "libkiwi.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
pub const LIB_FILE: &str = "libkiwi.so";
pub const MODEL_DIR: &str = "model";

#[repr(C)]
#[derive(Clone, Copy)]
struct AnalyzeOption {
    match_options: c_int,
    blocklist: Handle,
    open_ending: c_int,
    allowed_dialects: c_int,
    dialect_cost: c_float,
    typo_transformer: Handle,
    typo_threshold: c_float,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub integrate_allomorph: u8,
    pub cut_off_threshold: f32,
    pub oov_rule_scale: f32,
    pub oov_rule_bias: f32,
    pub oov_chr_bias: f32,
    pub oov_global_weight: f32,
    pub oov_local_weight: f32,
    pub oov_global_min_freq: f32,
    pub space_penalty: f32,
    pub typo_cost_weight: f32,
    pub max_unk_form_size: u32,
    pub max_unk_form_size_followed_by_j_class: u32,
    pub space_tolerance: u32,
}

struct Api {
    version: unsafe extern "C" fn() -> *const c_char,
    error: unsafe extern "C" fn() -> *const c_char,
    init: unsafe extern "C" fn(*const c_char, c_int, c_int, c_int) -> Handle,
    close: unsafe extern "C" fn(Handle) -> c_int,
    get_config: unsafe extern "C" fn(Handle) -> Config,
    set_config: unsafe extern "C" fn(Handle, Config),
    space: unsafe extern "C" fn(Handle, *const c_char, c_int) -> *const c_char,
    free_string: unsafe extern "C" fn(*const c_char) -> c_int,
    analyze: unsafe extern "C" fn(Handle, *const c_char, c_int, AnalyzeOption, Handle) -> Handle,
    res_size: unsafe extern "C" fn(Handle) -> c_int,
    res_word_num: unsafe extern "C" fn(Handle, c_int) -> c_int,
    res_prob: unsafe extern "C" fn(Handle, c_int) -> c_float,
    res_form: unsafe extern "C" fn(Handle, c_int, c_int) -> *const c_char,
    res_tag: unsafe extern "C" fn(Handle, c_int, c_int) -> *const c_char,
    res_position: unsafe extern "C" fn(Handle, c_int, c_int) -> c_int,
    res_length: unsafe extern "C" fn(Handle, c_int, c_int) -> c_int,
    res_word_position: unsafe extern "C" fn(Handle, c_int, c_int) -> c_int,
    res_typo_cost: unsafe extern "C" fn(Handle, c_int, c_int) -> c_float,
    res_close: unsafe extern "C" fn(Handle) -> c_int,
    typo_get_default: unsafe extern "C" fn(c_int) -> Handle,
    typo_prepare: unsafe extern "C" fn(Handle) -> Handle,
    prepared_typo_close: unsafe extern "C" fn(Handle) -> c_int,
    new_joiner: unsafe extern "C" fn(Handle, c_int) -> Handle,
    joiner_add: unsafe extern "C" fn(Handle, *const c_char, *const c_char, c_int) -> c_int,
    joiner_get: unsafe extern "C" fn(Handle) -> *const c_char,
    joiner_close: unsafe extern "C" fn(Handle) -> c_int,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub form: String,
    pub tag: String,
    pub position: usize,
    pub length: usize,
    pub word: usize,
    pub typo_cost: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    LibraryMissing(PathBuf),
    ModelMissing(PathBuf),
    Library(String),
    Symbol(String),
    Init(String),
}

impl LoadError {
    pub fn code(&self) -> &'static str {
        match self {
            LoadError::LibraryMissing(_) => "KIWI_LIBRARY_MISSING",
            LoadError::ModelMissing(_) => "KIWI_MODEL_MISSING",
            LoadError::Library(_) => "KIWI_LIBRARY_LOAD",
            LoadError::Symbol(_) => "KIWI_SYMBOL",
            LoadError::Init(_) => "KIWI_INIT",
        }
    }
}

pub struct KiwiLib {
    api: Api,
    _lib: libloading::Library,
}

pub struct Kiwi {
    lib: Arc<KiwiLib>,
    handle: Handle,
    typo: Handle,
}

unsafe impl Send for Kiwi {}

fn text_of(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
    }
}

#[cfg(windows)]
fn open_library(path: &Path) -> Result<libloading::Library, libloading::Error> {
    unsafe { libloading::os::windows::Library::load_with_flags(path, libloading::os::windows::LOAD_WITH_ALTERED_SEARCH_PATH) }.map(Into::into)
}

#[cfg(not(windows))]
fn open_library(path: &Path) -> Result<libloading::Library, libloading::Error> {
    unsafe { libloading::Library::new(path) }
}

macro_rules! symbols {
    ($lib:expr, $($field:ident = $name:literal),* $(,)?) => {
        Api { $($field: *unsafe { $lib.get($name) }.map_err(|e| LoadError::Symbol(format!("{}: {e}", String::from_utf8_lossy(&$name[..$name.len() - 1]))))?,)* }
    };
}

impl KiwiLib {
    pub fn open(dir: &Path) -> Result<Arc<KiwiLib>, LoadError> {
        let lib_path = dir.join(LIB_FILE);
        if !lib_path.is_file() {
            return Err(LoadError::LibraryMissing(lib_path));
        }
        let lib = open_library(&lib_path).map_err(|e| LoadError::Library(e.to_string()))?;
        let api = symbols!(lib,
            version = b"kiwi_version\0",
            error = b"kiwi_error\0",
            init = b"kiwi_init\0",
            close = b"kiwi_close\0",
            get_config = b"kiwi_get_global_config\0",
            set_config = b"kiwi_set_global_config\0",
            space = b"kiwi_space\0",
            free_string = b"kiwi_free_string\0",
            analyze = b"kiwi_analyze\0",
            res_size = b"kiwi_res_size\0",
            res_word_num = b"kiwi_res_word_num\0",
            res_prob = b"kiwi_res_prob\0",
            res_form = b"kiwi_res_form\0",
            res_tag = b"kiwi_res_tag\0",
            res_position = b"kiwi_res_position\0",
            res_length = b"kiwi_res_length\0",
            res_word_position = b"kiwi_res_word_position\0",
            res_typo_cost = b"kiwi_res_typo_cost\0",
            res_close = b"kiwi_res_close\0",
            typo_get_default = b"kiwi_typo_get_default\0",
            typo_prepare = b"kiwi_typo_prepare\0",
            prepared_typo_close = b"kiwi_prepared_typo_close\0",
            new_joiner = b"kiwi_new_joiner\0",
            joiner_add = b"kiwi_joiner_add\0",
            joiner_get = b"kiwi_joiner_get\0",
            joiner_close = b"kiwi_joiner_close\0",
        );
        Ok(Arc::new(KiwiLib { api, _lib: lib }))
    }
}

impl Kiwi {
    pub fn load(dir: &Path, threads: i32, typo_cost_weight: f32) -> Result<Kiwi, LoadError> {
        let lib = KiwiLib::open(dir)?;
        Kiwi::create(lib, dir, threads, typo_cost_weight)
    }

    pub fn create(lib: Arc<KiwiLib>, dir: &Path, threads: i32, typo_cost_weight: f32) -> Result<Kiwi, LoadError> {
        let model = dir.join(MODEL_DIR);
        if !model.is_dir() {
            return Err(LoadError::ModelMissing(model));
        }
        let api = &lib.api;
        let model_c = CString::new(model.to_string_lossy().into_owned()).map_err(|e| LoadError::Init(e.to_string()))?;
        let handle = unsafe { (api.init)(model_c.as_ptr(), threads, BUILD_DEFAULT, 0) };
        if handle.is_null() {
            let message = text_of(unsafe { (api.error)() });
            return Err(LoadError::Init(message));
        }
        let typo = unsafe {
            let set = (api.typo_get_default)(TYPO_BASIC);
            if set.is_null() { std::ptr::null_mut() } else { (api.typo_prepare)(set) }
        };
        if typo_cost_weight > 0.0 {
            unsafe {
                let mut config = (api.get_config)(handle);
                config.typo_cost_weight = typo_cost_weight;
                (api.set_config)(handle, config);
            }
        }
        Ok(Kiwi { lib, handle, typo })
    }

    pub fn version(&self) -> String {
        text_of(unsafe { (self.lib.api.version)() })
    }

    pub fn config(&self) -> Config {
        unsafe { (self.lib.api.get_config)(self.handle) }
    }

    pub fn has_typo_model(&self) -> bool {
        !self.typo.is_null()
    }

    pub fn space(&self, text: &str, reset_whitespace: bool) -> Option<String> {
        let input = CString::new(text).ok()?;
        let out = unsafe { (self.lib.api.space)(self.handle, input.as_ptr(), reset_whitespace as c_int) };
        if out.is_null() {
            return None;
        }
        let spaced = text_of(out);
        unsafe { (self.lib.api.free_string)(out) };
        Some(spaced)
    }

    pub fn analyze(&self, text: &str, typos: bool, typo_threshold: f32) -> Vec<Token> {
        self.analyze_scored(text, typos, typo_threshold).1
    }

    pub fn score(&self, text: &str) -> f32 {
        self.analyze_scored(text, false, 0.0).0
    }

    pub fn analyze_scored(&self, text: &str, typos: bool, typo_threshold: f32) -> (f32, Vec<Token>) {
        let Ok(input) = CString::new(text) else { return (f32::NEG_INFINITY, Vec::new()) };
        let option = AnalyzeOption {
            match_options: MATCH_ALL_WITH_NORMALIZING,
            blocklist: std::ptr::null_mut(),
            open_ending: 0,
            allowed_dialects: 0,
            dialect_cost: 3.0,
            typo_transformer: if typos { self.typo } else { std::ptr::null_mut() },
            typo_threshold,
        };
        let res = unsafe { (self.lib.api.analyze)(self.handle, input.as_ptr(), 1, option, std::ptr::null_mut()) };
        if res.is_null() {
            return (f32::NEG_INFINITY, Vec::new());
        }
        let mut out = Vec::new();
        let mut prob = f32::NEG_INFINITY;
        unsafe {
            if (self.lib.api.res_size)(res) > 0 {
                prob = (self.lib.api.res_prob)(res, 0);
                let count = (self.lib.api.res_word_num)(res, 0).max(0);
                for i in 0..count {
                    out.push(Token {
                        form: text_of((self.lib.api.res_form)(res, 0, i)),
                        tag: text_of((self.lib.api.res_tag)(res, 0, i)),
                        position: (self.lib.api.res_position)(res, 0, i).max(0) as usize,
                        length: (self.lib.api.res_length)(res, 0, i).max(0) as usize,
                        word: (self.lib.api.res_word_position)(res, 0, i).max(0) as usize,
                        typo_cost: (self.lib.api.res_typo_cost)(res, 0, i),
                    });
                }
            }
            (self.lib.api.res_close)(res);
        }
        (prob, out)
    }

    pub fn join(&self, morphs: &[(&str, &str)]) -> Option<String> {
        let owned: Vec<(CString, CString)> = morphs.iter().map(|(f, t)| Some((CString::new(*f).ok()?, CString::new(*t).ok()?))).collect::<Option<_>>()?;
        unsafe {
            let joiner = (self.lib.api.new_joiner)(self.handle, 1);
            if joiner.is_null() {
                return None;
            }
            for (form, tag) in &owned {
                (self.lib.api.joiner_add)(joiner, form.as_ptr(), tag.as_ptr(), 1);
            }
            let ptr = (self.lib.api.joiner_get)(joiner);
            let out = if ptr.is_null() { None } else { Some(text_of(ptr)) };
            (self.lib.api.joiner_close)(joiner);
            out
        }
    }
}

impl Drop for Kiwi {
    fn drop(&mut self) {
        unsafe {
            if !self.typo.is_null() {
                (self.lib.api.prepared_typo_close)(self.typo);
            }
            (self.lib.api.close)(self.handle);
        }
    }
}
