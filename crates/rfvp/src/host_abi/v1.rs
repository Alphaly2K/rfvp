//! C ABI v1 layout mirror.
//!
//! Keep this file byte-for-byte aligned with `doc/abi/rfvp_api_v1.h` in the
//! Art3m1s repository. This module intentionally contains no runtime adapter;
//! it is the stable type contract used by later implementation phases.

use core::mem::size_of;

pub const RFVP_API_ABI_VERSION: u32 = 1;
pub const RFVP_API_ABI_MAGIC: u64 = 0x4950_4131_5056_4652; // "RFVP1API"
pub const RFVP_INVALID_HANDLE: u64 = 0;

pub type RfvpStatusV1 = i32;

pub const RFVP_STATUS_OK: RfvpStatusV1 = 0;
pub const RFVP_STATUS_NO_FRAME: RfvpStatusV1 = 1;
pub const RFVP_STATUS_INVALID_ARGUMENT: RfvpStatusV1 = -1;
pub const RFVP_STATUS_INVALID_HANDLE: RfvpStatusV1 = -2;
pub const RFVP_STATUS_INVALID_STATE: RfvpStatusV1 = -3;
pub const RFVP_STATUS_NOT_FOUND: RfvpStatusV1 = -4;
pub const RFVP_STATUS_INVALID_DATA: RfvpStatusV1 = -5;
pub const RFVP_STATUS_UNSUPPORTED: RfvpStatusV1 = -6;
pub const RFVP_STATUS_OUT_OF_MEMORY: RfvpStatusV1 = -7;
pub const RFVP_STATUS_BUSY: RfvpStatusV1 = -8;
pub const RFVP_STATUS_IO: RfvpStatusV1 = -9;
pub const RFVP_STATUS_ENGINE: RfvpStatusV1 = -10;

pub const RFVP_NLS_SHIFT_JIS: u32 = 1;
pub const RFVP_NLS_GBK: u32 = 2;
pub const RFVP_NLS_UTF8: u32 = 3;

pub const RFVP_SERIALIZATION_UTF8_TEXT: u32 = 1;
pub const RFVP_SERIALIZATION_JSON: u32 = 2;
pub const RFVP_SERIALIZATION_BINARY_V1: u32 = 3;

pub const RFVP_EVENT_LOG: u32 = 1;
pub const RFVP_EVENT_MEDIA: u32 = 2;
pub const RFVP_EVENT_UI: u32 = 3;
pub const RFVP_EVENT_TEXT_TRANSLATION: u32 = 4;

pub const RFVP_INPUT_KEY: u32 = 1;
pub const RFVP_INPUT_TEXT: u32 = 2;
pub const RFVP_INPUT_POINTER_MOVE: u32 = 3;
pub const RFVP_INPUT_POINTER_BUTTON: u32 = 4;
pub const RFVP_INPUT_WHEEL: u32 = 5;
pub const RFVP_INPUT_TOUCH: u32 = 6;
pub const RFVP_INPUT_FOCUS: u32 = 7;
pub const RFVP_INPUT_QUIT: u32 = 8;

pub const RFVP_INPUT_PHASE_DOWN: u32 = 0;
pub const RFVP_INPUT_PHASE_UP: u32 = 1;
pub const RFVP_INPUT_PHASE_REPEAT: u32 = 2;
pub const RFVP_INPUT_PHASE_MOVE: u32 = 3;

pub const RFVP_POINTER_LEFT: u32 = 1 << 0;
pub const RFVP_POINTER_RIGHT: u32 = 1 << 1;
pub const RFVP_POINTER_MIDDLE: u32 = 1 << 2;

pub const RFVP_KEY_BACKSPACE: u32 = 8;
pub const RFVP_KEY_TAB: u32 = 9;
pub const RFVP_KEY_RETURN: u32 = 13;
pub const RFVP_KEY_ESCAPE: u32 = 27;
pub const RFVP_KEY_SPACE: u32 = 32;
pub const RFVP_KEY_PAGE_UP: u32 = 33;
pub const RFVP_KEY_PAGE_DOWN: u32 = 34;
pub const RFVP_KEY_END: u32 = 35;
pub const RFVP_KEY_HOME: u32 = 36;
pub const RFVP_KEY_LEFT: u32 = 37;
pub const RFVP_KEY_UP: u32 = 38;
pub const RFVP_KEY_RIGHT: u32 = 39;
pub const RFVP_KEY_DOWN: u32 = 40;
pub const RFVP_KEY_INSERT: u32 = 45;
pub const RFVP_KEY_DELETE: u32 = 46;
pub const RFVP_KEY_SHIFT: u32 = 16;
pub const RFVP_KEY_CONTROL: u32 = 17;
pub const RFVP_KEY_ALT: u32 = 18;

pub const RFVP_MODIFIER_SHIFT: u32 = 1 << 0;
pub const RFVP_MODIFIER_CONTROL: u32 = 1 << 1;
pub const RFVP_MODIFIER_ALT: u32 = 1 << 2;
pub const RFVP_MODIFIER_SUPER: u32 = 1 << 3;

pub const RFVP_TEXTURE_FORMAT_RGBA8: u32 = 1;
pub const RFVP_TEXTURE_FORMAT_LUMA_A8: u32 = 2;

pub const RFVP_TEXTURE_CREATE: u32 = 1;
pub const RFVP_TEXTURE_UPDATE: u32 = 2;
pub const RFVP_TEXTURE_DESTROY: u32 = 3;

pub const RFVP_BLEND_NORMAL: u32 = 0;
pub const RFVP_BLEND_ADD: u32 = 1;
pub const RFVP_BLEND_REVERSE_SUBTRACT: u32 = 2;
pub const RFVP_BLEND_MULTIPLY: u32 = 3;
pub const RFVP_BLEND_SCREEN: u32 = 4;

pub const RFVP_TEXTURE_FILTER_NEAREST: u32 = 0;
pub const RFVP_TEXTURE_FILTER_LINEAR: u32 = 1;

pub const RFVP_DRAW_IMAGE: u32 = 1;
pub const RFVP_DRAW_GLYPH: u32 = 2;
pub const RFVP_DRAW_SOLID: u32 = 3;

pub const RFVP_MESH_TRIANGLE_LIST: u32 = 1;
pub const RFVP_MESH_TRIANGLE_STRIP: u32 = 2;

pub const RFVP_LIFECYCLE_BACKGROUND: u32 = 1;
pub const RFVP_LIFECYCLE_FOREGROUND: u32 = 2;
pub const RFVP_LIFECYCLE_EXIT: u32 = 3;

pub const RFVP_RENDER_QUALITY_NATIVE: u32 = 0;
pub const RFVP_RENDER_QUALITY_QUALITY: u32 = 1;
pub const RFVP_RENDER_QUALITY_BALANCED: u32 = 2;
pub const RFVP_RENDER_QUALITY_PERFORMANCE: u32 = 3;

pub const RFVP_VOLUME_MASTER: u32 = 1;
pub const RFVP_VOLUME_BGM: u32 = 2;
pub const RFVP_VOLUME_SE: u32 = 3;
pub const RFVP_VOLUME_VOICE: u32 = 4;

pub const RFVP_DRAW_FLAG_HAS_CLIP: u32 = 1 << 0;
pub const RFVP_DRAW_FLAG_HAS_MESH: u32 = 1 << 1;
pub const RFVP_DRAW_FLAG_HAS_EFFECT: u32 = 1 << 2;
pub const RFVP_DRAW_FLAG_HAS_SRC_RECT: u32 = 1 << 3;

pub const RFVP_HIT_PROXY_ENABLED: u32 = 1 << 0;
pub const RFVP_HIT_PROXY_VISIBLE: u32 = 1 << 1;

pub const RFVP_CAPABILITY_EVENTS: u64 = 1 << 0;
pub const RFVP_CAPABILITY_TEXTURES: u64 = 1 << 1;
pub const RFVP_CAPABILITY_DRAW_IMAGE: u64 = 1 << 2;
pub const RFVP_CAPABILITY_DRAW_GLYPH: u64 = 1 << 3;
pub const RFVP_CAPABILITY_DRAW_MESH: u64 = 1 << 4;
pub const RFVP_CAPABILITY_DRAW_EFFECTS: u64 = 1 << 5;
pub const RFVP_CAPABILITY_HIT_PROXIES: u64 = 1 << 6;
pub const RFVP_CAPABILITY_TEXT_REPLACEMENTS: u64 = 1 << 7;
pub const RFVP_CAPABILITY_TEXT_TRANSLATION: u64 = 1 << 8;
pub const RFVP_CAPABILITY_NATIVE_SURFACE: u64 = 1 << 9;

pub const RFVP_TEXTURE_ID_WHITE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpResourcesConfigV1 {
    pub struct_size: u32,
    pub flags: u32,
    pub nls: u32,
    pub reserved0: u32,
    pub save_root_utf8: *const u8,
    pub save_root_len: usize,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpRuntimeConfigV1 {
    pub struct_size: u32,
    pub flags: u32,
    pub resources: u64,
    pub requested_width: u32,
    pub requested_height: u32,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpInputEventV1 {
    pub struct_size: u32,
    pub kind: u32,
    pub code: u32,
    pub phase: u32,
    pub x: i32,
    pub y: i32,
    pub value: i32,
    pub modifiers: u32,
    pub id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RfvpColorV1 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpRectU16V1 {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpRectI32V1 {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RfvpVertexV1 {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub color: RfvpColorV1,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpTextureCommandV1 {
    pub struct_size: u32,
    pub kind: u32,
    pub texture_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub row_bytes: u32,
    pub rect: RfvpRectI32V1,
    pub generation: u64,
    pub pixels: *const u8,
    pub pixels_size: usize,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RfvpDrawCommandV1 {
    pub struct_size: u32,
    pub kind: u32,
    pub flags: u32,
    pub texture_id: u32,
    pub blend: u32,
    pub filter: u32,
    pub effect_id: u32,
    pub mesh_topology: u32,
    pub src_rect: RfvpRectU16V1,
    pub dst_rect: RfvpRectI32V1,
    pub clip_rect: RfvpRectI32V1,
    pub color: RfvpColorV1,
    pub vertices: [RfvpVertexV1; 4],
    pub mesh: *const RfvpVertexV1,
    pub mesh_vertex_count: usize,
    pub effect_data: *const u8,
    pub effect_data_size: usize,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpHitProxyV1 {
    pub prim_id: u32,
    pub flags: u32,
    pub rect: RfvpRectI32V1,
    pub order: u32,
    pub reserved0: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpEventHeaderV1 {
    pub abi_version: u32,
    pub kind: u32,
    pub sequence: u64,
    pub payload_size: u32,
    pub aux: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RfvpTextTranslationEventV1 {
    pub struct_size: u32,
    pub encoding: u32,
    pub serial: u64,
    pub generation: u64,
    pub slot: u32,
    pub source_offset: u32,
    pub source_len: u32,
    pub ruby_offset: u32,
    pub ruby_len: u32,
    pub reserved0: u32,
}

pub type RfvpResourcesCreateFn =
    unsafe extern "C" fn(config: *const RfvpResourcesConfigV1, out_resources: *mut u64) -> i32;
pub type RfvpResourcesDestroyFn = unsafe extern "C" fn(resources: u64);
pub type RfvpResourcesClearFn = unsafe extern "C" fn(resources: u64);
pub type RfvpResourcesMountDirectoryFn =
    unsafe extern "C" fn(resources: u64, path_utf8: *const u8, path_len: usize) -> i32;
pub type RfvpResourcesMountPackFn = unsafe extern "C" fn(
    resources: u64,
    folder_utf8: *const u8,
    folder_len: usize,
    pack_data: *const u8,
    pack_size: usize,
) -> i32;
pub type RfvpResourcesSetOverrideFn = unsafe extern "C" fn(
    resources: u64,
    path_utf8: *const u8,
    path_len: usize,
    data: *const u8,
    data_size: usize,
) -> i32;
pub type RfvpResourcesClearOverridesFn = unsafe extern "C" fn(resources: u64);
pub type RfvpResourcesSetSaveRootFn =
    unsafe extern "C" fn(resources: u64, path_utf8: *const u8, path_len: usize) -> i32;

pub type RfvpRuntimeCreateFn =
    unsafe extern "C" fn(config: *const RfvpRuntimeConfigV1, out_runtime: *mut u64) -> i32;
pub type RfvpRuntimeDestroyFn = unsafe extern "C" fn(runtime: u64);
pub type RfvpRuntimeStepFn = unsafe extern "C" fn(runtime: u64, delta_ms: u32) -> i32;
pub type RfvpRuntimeIsExitRequestedFn = unsafe extern "C" fn(runtime: u64) -> i32;
pub type RfvpRuntimeEventsEnableFn = unsafe extern "C" fn(runtime: u64, enabled: i32) -> i32;
pub type RfvpRuntimeNextEventSizeFn = unsafe extern "C" fn(runtime: u64) -> usize;
pub type RfvpRuntimePollEventsFn =
    unsafe extern "C" fn(runtime: u64, out: *mut u8, capacity: usize, out_count: *mut u32) -> usize;
pub type RfvpRuntimePushInputFn =
    unsafe extern "C" fn(runtime: u64, events: *const RfvpInputEventV1, event_count: usize) -> i32;
pub type RfvpRuntimeSetTextHidpiFn = unsafe extern "C" fn(runtime: u64, enabled: i32) -> i32;
pub type RfvpRuntimeSetTextReplacementsFn =
    unsafe extern "C" fn(runtime: u64, blob: *const u8, blob_size: usize, encoding: u32) -> i32;
pub type RfvpRuntimeSetTextTranslationEnabledFn =
    unsafe extern "C" fn(runtime: u64, enabled: i32) -> i32;
pub type RfvpRuntimeSubmitTextTranslationFn = unsafe extern "C" fn(
    runtime: u64,
    serial: u64,
    translated_utf8: *const u8,
    translated_len: usize,
) -> i32;
pub type RfvpRuntimeSetRenderQualityPresetFn =
    unsafe extern "C" fn(runtime: u64, preset: i32) -> i32;
pub type RfvpRuntimeSetMediaEnabledFn = unsafe extern "C" fn(runtime: u64, enabled: i32) -> i32;
pub type RfvpRuntimeNotifyLifecycleFn = unsafe extern "C" fn(runtime: u64, state: i32) -> i32;
pub type RfvpRuntimeSetVolumeFn =
    unsafe extern "C" fn(runtime: u64, channel: u32, value: f32) -> i32;
pub type RfvpRuntimeCapabilitiesFn = unsafe extern "C" fn(runtime: u64) -> u64;
pub type RfvpRuntimeAcquireFrameFn = unsafe extern "C" fn(runtime: u64, out_frame: *mut u64) -> i32;

pub type RfvpFrameReleaseFn = unsafe extern "C" fn(frame: u64);
pub type RfvpFrameGetSizeFn =
    unsafe extern "C" fn(frame: u64, out_width: *mut u32, out_height: *mut u32) -> i32;
pub type RfvpFrameGetCommandsFn = unsafe extern "C" fn(
    frame: u64,
    out_commands: *mut *const RfvpDrawCommandV1,
    out_count: *mut usize,
) -> i32;
pub type RfvpFrameGetTexturesFn = unsafe extern "C" fn(
    frame: u64,
    out_commands: *mut *const RfvpTextureCommandV1,
    out_count: *mut usize,
) -> i32;
pub type RfvpFrameGetHitProxiesFn = unsafe extern "C" fn(
    frame: u64,
    out_proxies: *mut *const RfvpHitProxyV1,
    out_count: *mut usize,
) -> i32;

#[repr(C)]
pub struct RfvpApiV1 {
    pub struct_size: u32,
    pub abi_version: u32,
    pub magic: u64,

    pub resources_create: Option<RfvpResourcesCreateFn>,
    pub resources_destroy: Option<RfvpResourcesDestroyFn>,
    pub resources_clear: Option<RfvpResourcesClearFn>,
    pub resources_mount_directory: Option<RfvpResourcesMountDirectoryFn>,
    pub resources_mount_pack: Option<RfvpResourcesMountPackFn>,
    pub resources_set_override: Option<RfvpResourcesSetOverrideFn>,
    pub resources_clear_overrides: Option<RfvpResourcesClearOverridesFn>,
    pub resources_set_save_root: Option<RfvpResourcesSetSaveRootFn>,

    pub runtime_create: Option<RfvpRuntimeCreateFn>,
    pub runtime_destroy: Option<RfvpRuntimeDestroyFn>,
    pub runtime_step: Option<RfvpRuntimeStepFn>,
    pub runtime_is_exit_requested: Option<RfvpRuntimeIsExitRequestedFn>,
    pub runtime_events_enable: Option<RfvpRuntimeEventsEnableFn>,
    pub runtime_next_event_size: Option<RfvpRuntimeNextEventSizeFn>,
    pub runtime_poll_events: Option<RfvpRuntimePollEventsFn>,
    pub runtime_push_input: Option<RfvpRuntimePushInputFn>,
    pub runtime_set_text_hidpi: Option<RfvpRuntimeSetTextHidpiFn>,
    pub runtime_set_text_replacements: Option<RfvpRuntimeSetTextReplacementsFn>,
    pub runtime_set_text_translation_enabled: Option<RfvpRuntimeSetTextTranslationEnabledFn>,
    pub runtime_submit_text_translation: Option<RfvpRuntimeSubmitTextTranslationFn>,
    pub runtime_set_render_quality_preset: Option<RfvpRuntimeSetRenderQualityPresetFn>,
    pub runtime_set_media_enabled: Option<RfvpRuntimeSetMediaEnabledFn>,
    pub runtime_notify_lifecycle: Option<RfvpRuntimeNotifyLifecycleFn>,
    pub runtime_set_volume: Option<RfvpRuntimeSetVolumeFn>,
    pub runtime_capabilities: Option<RfvpRuntimeCapabilitiesFn>,
    pub runtime_acquire_frame: Option<RfvpRuntimeAcquireFrameFn>,

    pub frame_release: Option<RfvpFrameReleaseFn>,
    pub frame_get_size: Option<RfvpFrameGetSizeFn>,
    pub frame_get_commands: Option<RfvpFrameGetCommandsFn>,
    pub frame_get_textures: Option<RfvpFrameGetTexturesFn>,
    pub frame_get_hit_proxies: Option<RfvpFrameGetHitProxiesFn>,
}

static API_V1: RfvpApiV1 = RfvpApiV1 {
    struct_size: size_of::<RfvpApiV1>() as u32,
    abi_version: RFVP_API_ABI_VERSION,
    magic: RFVP_API_ABI_MAGIC,
    resources_create: None,
    resources_destroy: None,
    resources_clear: None,
    resources_mount_directory: None,
    resources_mount_pack: None,
    resources_set_override: None,
    resources_clear_overrides: None,
    resources_set_save_root: None,
    runtime_create: None,
    runtime_destroy: None,
    runtime_step: None,
    runtime_is_exit_requested: None,
    runtime_events_enable: None,
    runtime_next_event_size: None,
    runtime_poll_events: None,
    runtime_push_input: None,
    runtime_set_text_hidpi: None,
    runtime_set_text_replacements: None,
    runtime_set_text_translation_enabled: None,
    runtime_submit_text_translation: None,
    runtime_set_render_quality_preset: None,
    runtime_set_media_enabled: None,
    runtime_notify_lifecycle: None,
    runtime_set_volume: None,
    runtime_capabilities: None,
    runtime_acquire_frame: None,
    frame_release: None,
    frame_get_size: None,
    frame_get_commands: None,
    frame_get_textures: None,
    frame_get_hit_proxies: None,
};

/// Returns the process-lifetime v1 table used by the future flat export.
pub fn api_v1_table() -> &'static RfvpApiV1 {
    &API_V1
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn api_identity_is_stable() {
        let api = api_v1_table();
        assert_eq!(api.abi_version, RFVP_API_ABI_VERSION);
        assert_eq!(api.magic, RFVP_API_ABI_MAGIC);
        assert_eq!(api.struct_size as usize, size_of::<RfvpApiV1>());
    }

    #[test]
    fn fixed_layout_offsets_match_header() {
        assert_eq!(size_of::<RfvpEventHeaderV1>(), 24);
        assert_eq!(offset_of!(RfvpEventHeaderV1, sequence), 8);
        assert_eq!(offset_of!(RfvpEventHeaderV1, payload_size), 16);

        assert_eq!(offset_of!(RfvpTextTranslationEventV1, serial), 8);
        assert_eq!(offset_of!(RfvpTextTranslationEventV1, generation), 16);
        assert_eq!(offset_of!(RfvpTextTranslationEventV1, source_offset), 28);
        assert_eq!(offset_of!(RfvpTextTranslationEventV1, ruby_offset), 36);

        assert_eq!(size_of::<RfvpVertexV1>(), 32);
        assert_eq!(offset_of!(RfvpVertexV1, u), 8);
        assert_eq!(offset_of!(RfvpVertexV1, color), 16);

        assert_eq!(offset_of!(RfvpDrawCommandV1, src_rect), 32);
        assert_eq!(offset_of!(RfvpDrawCommandV1, vertices), 88);
    }

    #[test]
    fn table_starts_without_implemented_entries() {
        let api = api_v1_table();
        assert!(api.resources_create.is_none());
        assert!(api.runtime_create.is_none());
        assert!(api.frame_release.is_none());
    }
}
