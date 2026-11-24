use dtool_protocol::CommandError;
use crate::colour::OcioConfig;

#[repr(C)]
struct OcioProcessor {
    _opaque: [u8; 0],
}

#[cfg(feature = "ocio")]
unsafe extern "C" {
    fn ocio_engine_open(config_path: *const i8) -> *mut OcioEngineHandle;
    fn ocio_engine_close(handle: *mut OcioEngineHandle);
    fn ocio_apply(handle: *mut OcioEngineHandle, src_space: *const i8, dst_space: *const i8, pixels: *mut f32, count: usize) -> i32;
}

#[repr(C)]
struct OcioEngineHandle {
    _opaque: [u8; 0],
}

pub struct OcioEngine {
    handle: *mut OcioEngineHandle,
}

#[cfg(feature = "ocio")]
impl OcioEngine {
    pub fn open(config: &OcioConfig) -> Result<Self, CommandError> {
        let path = config.path.to_string_lossy();
        let c_path = std::ffi::CString::new(path.as_ref()).map_err(|_| CommandError::invalid_params())?;
        
        unsafe {
            let handle = ocio_engine_open(c_path.as_ptr());
            if handle.is_null() {
                return Err(CommandError::validation("OCIO_CONFIG_INVALID"));
            }
            Ok(OcioEngine { handle })
        }
    }

    pub fn apply(&self, src: &str, dst: &str, px: &mut [f32]) -> Result<(), CommandError> {
        let c_src = std::ffi::CString::new(src).map_err(|_| CommandError::invalid_params())?;
        let c_dst = std::ffi::CString::new(dst).map_err(|_| CommandError::invalid_params())?;
        
        unsafe {
            let result = ocio_apply(self.handle, c_src.as_ptr(), c_dst.as_ptr(), px.as_mut_ptr(), px.len());
            if result != 0 {
                return Err(CommandError::validation("OCIO_CONFIG_INVALID"));
            }
            Ok(())
        }
    }
}

#[cfg(feature = "ocio")]
impl Drop for OcioEngine {
    fn drop(&mut self) {
        unsafe {
            ocio_engine_close(self.handle);
        }
    }
}

pub const OCIO_NOT_AVAILABLE: &str = "OCIO_NOT_AVAILABLE";

#[cfg(not(feature = "ocio"))]
impl OcioEngine {
    pub fn open(config: &OcioConfig) -> Result<Self, CommandError> {
        let _ = (&config.path, std::ptr::null_mut::<OcioEngineHandle>());
        Err(CommandError::validation(OCIO_NOT_AVAILABLE))
    }

    pub fn apply(&self, src: &str, dst: &str, px: &mut [f32]) -> Result<(), CommandError> {
        let _ = (self.handle, src, dst, px.len());
        Err(CommandError::validation(OCIO_NOT_AVAILABLE))
    }
}

#[cfg(all(test, not(feature = "ocio")))]
mod stub_contracts {
    use super::*;

    #[test]
    fn open_without_ocio_feature_reports_not_available() {
        let config = OcioConfig { path: std::path::PathBuf::from("missing.ocio"), spaces: Vec::new() };
        let error = OcioEngine::open(&config).err().expect("stub must not open");
        assert_eq!(error.code, OCIO_NOT_AVAILABLE);
        assert_eq!(error.message_key, "error.ocio_not_available");
    }
}