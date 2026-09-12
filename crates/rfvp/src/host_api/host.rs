use super::audio::RfvpAudio;
use super::clock::RfvpClock;
use super::fs::RfvpFileSystem;
use super::render::RfvpRenderer;

#[cfg(any(feature = "no_std", feature = "host-runtime"))]
use core::ffi::c_void;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfvpLogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[cfg(any(feature = "no_std", feature = "host-runtime"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FatalErrorCode {
    MissingDefaultFont = 1,
    InvalidDefaultFont = 2,
}

#[cfg(any(feature = "no_std", feature = "host-runtime"))]
pub type FatalErrorCallback = extern "C" fn(
    user_data: *mut c_void,
    code: FatalErrorCode,
    message_ptr: *const u8,
    message_len: usize,
);

#[cfg(any(feature = "no_std", feature = "host-runtime"))]
#[derive(Debug, Clone, Copy)]
pub struct PlatformCallbacks {
    pub user_data: *mut c_void,
    pub fatal_error: Option<FatalErrorCallback>,
}

#[cfg(any(feature = "no_std", feature = "host-runtime"))]
impl Default for PlatformCallbacks {
    fn default() -> Self {
        Self {
            user_data: core::ptr::null_mut(),
            fatal_error: None,
        }
    }
}

pub trait RfvpHost {
    type FileSystem: RfvpFileSystem;
    type Renderer: RfvpRenderer;
    type Audio: RfvpAudio;
    type Clock: RfvpClock;

    fn fs(&mut self) -> &mut Self::FileSystem;

    fn renderer(&mut self) -> &mut Self::Renderer;

    fn audio(&mut self) -> &mut Self::Audio;

    fn clock(&mut self) -> &mut Self::Clock;

    fn log(&mut self, _level: RfvpLogLevel, _message: &str) {}

    #[cfg(any(feature = "no_std", feature = "host-runtime"))]
    fn platform_callbacks(&mut self) -> PlatformCallbacks {
        PlatformCallbacks::default()
    }
}
