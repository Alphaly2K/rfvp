//! Runtime implementation for the versioned host ABI.
//!
//! This path is deliberately windowless and device-owning only where required:
//! the engine produces backend-neutral render commands and audio commands, and
//! the Host owns presentation, GPU resources, CoreAudio/WASAPI/ALSA, and
//! lifecycle.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Component, Path, PathBuf};

use crate::host_abi::v1::{
    RfvpAudioCommandV1, RfvpColorV1, RfvpDrawCommandV1, RfvpHitProxyV1, RfvpInputEventV1,
    RfvpRectI32V1, RfvpRectU16V1, RfvpResourcesConfigV1, RfvpRuntimeConfigV1, RfvpTextureCommandV1,
    RfvpVertexV1, RFVP_AUDIO_CREATE_STREAM, RFVP_AUDIO_DESTROY_STREAM, RFVP_AUDIO_ENCODED_FLAC,
    RFVP_AUDIO_ENCODED_MP3, RFVP_AUDIO_ENCODED_OGG, RFVP_AUDIO_ENCODED_UNKNOWN,
    RFVP_AUDIO_ENCODED_WAV, RFVP_AUDIO_LOAD_ENCODED, RFVP_AUDIO_MASTER_VOLUME, RFVP_AUDIO_PAUSE,
    RFVP_AUDIO_PLAY, RFVP_AUDIO_RESUME, RFVP_AUDIO_SAMPLE_F32, RFVP_AUDIO_SAMPLE_I16,
    RFVP_AUDIO_SET_PARAMS, RFVP_AUDIO_STOP, RFVP_AUDIO_SUBMIT_F32, RFVP_AUDIO_SUBMIT_I16,
    RFVP_BLEND_ADD, RFVP_BLEND_MULTIPLY, RFVP_BLEND_NORMAL, RFVP_BLEND_REVERSE_SUBTRACT,
    RFVP_CAPABILITY_AUDIO_COMMANDS, RFVP_CAPABILITY_DRAW_GLYPH, RFVP_CAPABILITY_DRAW_IMAGE,
    RFVP_CAPABILITY_HIT_PROXIES, RFVP_CAPABILITY_TEXTURES, RFVP_DRAW_FLAG_HAS_CLIP,
    RFVP_DRAW_FLAG_HAS_SRC_RECT, RFVP_DRAW_GLYPH, RFVP_DRAW_IMAGE, RFVP_INPUT_FOCUS,
    RFVP_INPUT_KEY, RFVP_INPUT_PHASE_DOWN, RFVP_INPUT_PHASE_MOVE, RFVP_INPUT_PHASE_REPEAT,
    RFVP_INPUT_PHASE_UP, RFVP_INPUT_POINTER_BUTTON, RFVP_INPUT_POINTER_MOVE, RFVP_INPUT_QUIT,
    RFVP_INPUT_TEXT, RFVP_INPUT_TOUCH, RFVP_INPUT_WHEEL, RFVP_INVALID_HANDLE, RFVP_KEY_ALT,
    RFVP_KEY_BACKSPACE, RFVP_KEY_CONTROL, RFVP_KEY_DELETE, RFVP_KEY_DOWN, RFVP_KEY_END,
    RFVP_KEY_ESCAPE, RFVP_KEY_HOME, RFVP_KEY_INSERT, RFVP_KEY_LEFT, RFVP_KEY_PAGE_DOWN,
    RFVP_KEY_PAGE_UP, RFVP_KEY_RETURN, RFVP_KEY_RIGHT, RFVP_KEY_SHIFT, RFVP_KEY_SPACE,
    RFVP_KEY_TAB, RFVP_KEY_UP, RFVP_MESH_TRIANGLE_LIST, RFVP_NLS_GBK, RFVP_NLS_SHIFT_JIS,
    RFVP_NLS_UTF8, RFVP_POINTER_LEFT, RFVP_POINTER_MIDDLE, RFVP_POINTER_RIGHT, RFVP_STATUS_BUSY,
    RFVP_STATUS_ENGINE, RFVP_STATUS_INVALID_ARGUMENT, RFVP_STATUS_INVALID_DATA,
    RFVP_STATUS_INVALID_HANDLE, RFVP_STATUS_NOT_FOUND, RFVP_STATUS_NO_COMMAND,
    RFVP_STATUS_NO_FRAME, RFVP_STATUS_OK, RFVP_STATUS_UNSUPPORTED, RFVP_TEXTURE_CREATE,
    RFVP_TEXTURE_FILTER_LINEAR, RFVP_TEXTURE_FORMAT_LUMA_A8, RFVP_TEXTURE_FORMAT_RGBA8,
};
use crate::host_abi::{Handle, HandleRegistry};
use crate::host_api::{
    AudioParams, AudioSampleFormat, AudioStreamDesc, AudioStreamId, BlendMode, ColorRgba,
    CommandBlendMode, DrawGlyphCmd, DrawImageCmd, DrawSolidCommand, DrawSpriteCommand,
    EncodedAudioKind, InputModifiers, KeyCode, PixelFormat, PointerButton, PortableTextureDesc,
    RectI16, RectU16, RenderBackend, RenderCommand, RfvpAudio, RfvpClock, RfvpError, RfvpEvent,
    RfvpFile, RfvpFileInfo, RfvpFileKind, RfvpFileSystem, RfvpHost, RfvpLogLevel, RfvpRenderer,
    RfvpResult, Rgba8, TextureBackend, TextureDesc, TextureFormat, TextureHandle, TextureId,
    TextureRect, Vertex2D,
};
use crate::no_std_core::{RfvpBootConfig, RfvpCore, RfvpCoreConfig};
use crate::rendering::external::{
    ExternalFrame, RecordedTextureCommand, RecordedTextureCreate, RecordedTextureUpdate,
    RecordingBackend,
};
use crate::rfvp_audio::AudioCommand;
use crate::script::parser::Nls;

thread_local! {
    static HOST_STATE: RefCell<HostState> = RefCell::new(HostState::default());
}

#[derive(Default)]
struct HostState {
    resources: HandleRegistry<HostResources>,
    runtimes: HandleRegistry<HostRuntime>,
    frames: HandleRegistry<HostFrame>,
}

struct HostResources {
    nls: Nls,
    game_root: Option<String>,
    save_root: Option<String>,
}

struct HostRuntime {
    core: RfvpCore,
    host: HostPlatform,
    pending_frame: Option<ExternalFrame>,
    audio_commands: VecDeque<AudioCommand>,
    pending_audio_command: Option<PendingAudioCommand>,
    audio_queue_overflowed: bool,
    width: u32,
    height: u32,
    exit_requested: bool,
    active_frame: Option<u64>,
}

struct HostFrame {
    width: u32,
    height: u32,
    runtime: u64,
    commands: Vec<RfvpDrawCommandV1>,
    textures: Vec<RfvpTextureCommandV1>,
    hit_proxies: Vec<RfvpHitProxyV1>,
    _texture_pixels: Vec<Vec<u8>>,
}

const MAX_PENDING_AUDIO_COMMANDS: usize = 1024;

struct PendingAudioCommand {
    command: RfvpAudioCommandV1,
    payload: Vec<u8>,
}

struct HostPlatform {
    filesystem: HostFileSystem,
    renderer: HostRenderer,
    audio: HostAudio,
    clock: HostClock,
}

impl HostPlatform {
    fn new(game_root: &str) -> Self {
        Self {
            filesystem: HostFileSystem::new(game_root),
            renderer: HostRenderer::default(),
            audio: HostAudio::default(),
            clock: HostClock::new(),
        }
    }
}

impl RfvpHost for HostPlatform {
    type FileSystem = HostFileSystem;
    type Renderer = HostRenderer;
    type Audio = HostAudio;
    type Clock = HostClock;

    fn fs(&mut self) -> &mut Self::FileSystem {
        &mut self.filesystem
    }

    fn renderer(&mut self) -> &mut Self::Renderer {
        &mut self.renderer
    }

    fn audio(&mut self) -> &mut Self::Audio {
        &mut self.audio
    }

    fn clock(&mut self) -> &mut Self::Clock {
        &mut self.clock
    }

    fn log(&mut self, level: RfvpLogLevel, message: &str) {
        match level {
            RfvpLogLevel::Error => log::error!("{message}"),
            RfvpLogLevel::Warn => log::warn!("{message}"),
            RfvpLogLevel::Info => log::info!("{message}"),
            RfvpLogLevel::Debug => log::debug!("{message}"),
            RfvpLogLevel::Trace => log::trace!("{message}"),
        }
    }
}

#[derive(Default)]
struct HostRenderer {
    backend: RecordingBackend,
    generations: HashMap<TextureHandle, u64>,
    textures: HashMap<TextureHandle, RecordedTextureCreate>,
    white_ready: bool,
}

impl HostRenderer {
    fn take_external_frame(&mut self) -> ExternalFrame {
        let texture_commands = std::mem::take(&mut self.backend.texture_commands);
        ExternalFrame {
            frame: std::mem::take(&mut self.backend.frame),
            textures: std::mem::take(&mut self.backend.creates),
            texture_commands,
        }
    }

    fn next_generation(&mut self, handle: TextureHandle) -> u64 {
        let generation = self.generations.entry(handle).or_insert(0);
        *generation = generation.wrapping_add(1).max(1);
        *generation
    }

    fn ensure_white(&mut self) -> RfvpResult<()> {
        if self.white_ready {
            return Ok(());
        }
        let handle = TextureHandle(u32::MAX);
        self.backend.create_texture(
            handle,
            PortableTextureDesc {
                width: 1,
                height: 1,
                format: TextureFormat::Rgba8,
            },
            &[255, 255, 255, 255],
        )?;
        let generation = self.next_generation(handle);
        if let Some(texture) = self.backend.creates.last_mut() {
            texture.generation = generation;
            self.textures.insert(handle, texture.clone());
        }
        self.white_ready = true;
        Ok(())
    }

    fn draw_sprite_direct(&mut self, command: &DrawSpriteCommand) -> RfvpResult<()> {
        let vertices = command.vertices;
        let mut min_x = vertices[0].position[0];
        let mut max_x = min_x;
        let mut min_y = vertices[0].position[1];
        let mut max_y = min_y;
        for vertex in vertices.iter().skip(1) {
            min_x = min_x.min(vertex.position[0]);
            max_x = max_x.max(vertex.position[0]);
            min_y = min_y.min(vertex.position[1]);
            max_y = max_y.max(vertex.position[1]);
        }
        let draw = DrawImageCmd {
            texture: TextureHandle(command.texture.0),
            src: RectU16::default(),
            dst: RectI16 {
                x: min_x.floor().clamp(i16::MIN as f32, i16::MAX as f32) as i16,
                y: min_y.floor().clamp(i16::MIN as f32, i16::MAX as f32) as i16,
                w: (max_x.ceil() - min_x.floor()).clamp(0.0, i16::MAX as f32) as i16,
                h: (max_y.ceil() - min_y.floor()).clamp(0.0, i16::MAX as f32) as i16,
            },
            color: rgba8(vertices[0].color),
            blend: command_blend(command.blend),
            effect_id: 0,
            clip: command.scissor.map(|rect| RectI16 {
                x: rect.x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                y: rect.y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                w: rect.width.clamp(0, i16::MAX as i32) as i16,
                h: rect.height.clamp(0, i16::MAX as i32) as i16,
            }),
            vertices,
        };
        self.backend
            .submit_commands(&[RenderCommand::DrawImage(draw)])
    }
}

impl RfvpRenderer for HostRenderer {
    fn create_texture(
        &mut self,
        id: TextureId,
        desc: TextureDesc,
        pixels: Option<&[u8]>,
    ) -> RfvpResult<()> {
        let handle = TextureHandle(id.0);
        let format = match desc.format {
            PixelFormat::Rgba8 => TextureFormat::Rgba8,
            PixelFormat::LumaA8 => TextureFormat::LumaA8,
            _ => return Err(RfvpError::Unsupported),
        };
        let empty = [];
        self.backend.create_texture(
            handle,
            PortableTextureDesc {
                width: desc.width.min(u16::MAX as u32) as u16,
                height: desc.height.min(u16::MAX as u32) as u16,
                format,
            },
            pixels.unwrap_or(&empty),
        )?;
        let generation = self.next_generation(handle);
        if let Some(texture) = self.backend.creates.last_mut() {
            texture.generation = generation;
            self.textures.insert(handle, texture.clone());
        }
        Ok(())
    }

    fn update_texture(
        &mut self,
        id: TextureId,
        rect: TextureRect,
        pixels: &[u8],
    ) -> RfvpResult<()> {
        let handle = TextureHandle(id.0);
        let generation = self.next_generation(handle);
        let Some(texture) = self.textures.get_mut(&handle) else {
            return Err(RfvpError::NotFound);
        };
        let row_bytes = texture.desc.width as usize
            * match texture.desc.format {
                TextureFormat::Rgba8 => 4,
                TextureFormat::LumaA8 => 2,
                _ => return Err(RfvpError::Unsupported),
            };
        let rect_row_bytes = rect.width as usize
            * match texture.desc.format {
                TextureFormat::Rgba8 => 4,
                TextureFormat::LumaA8 => 2,
                _ => return Err(RfvpError::Unsupported),
            };
        if rect.width == 0
            || rect.height == 0
            || pixels.len() < rect_row_bytes * rect.height as usize
        {
            return Err(RfvpError::InvalidArgument);
        }
        for row in 0..rect.height as usize {
            let src = &pixels[row * rect_row_bytes..(row + 1) * rect_row_bytes];
            let pixel_bytes = match texture.desc.format {
                TextureFormat::Rgba8 => 4,
                TextureFormat::LumaA8 => 2,
                _ => return Err(RfvpError::Unsupported),
            };
            let dst_offset = (rect.y as usize + row) * row_bytes + rect.x as usize * pixel_bytes;
            let dst = texture
                .pixels
                .get_mut(dst_offset..dst_offset + rect_row_bytes)
                .ok_or(RfvpError::InvalidArgument)?;
            dst.copy_from_slice(src);
        }
        texture.generation = generation;
        self.backend.record_texture_update(RecordedTextureUpdate {
            handle,
            rect,
            format: texture.desc.format,
            pixels: pixels.to_vec(),
            generation,
        });
        Ok(())
    }

    fn destroy_texture(&mut self, id: TextureId) {
        let handle = TextureHandle(id.0);
        self.backend.destroy_texture(handle);
        self.generations.remove(&handle);
        self.textures.remove(&handle);
    }

    fn begin_frame(
        &mut self,
        width: u32,
        height: u32,
        _clear: Option<ColorRgba>,
    ) -> RfvpResult<()> {
        self.backend.begin_frame(
            u16::try_from(width).map_err(|_| RfvpError::CapacityExceeded)?,
            u16::try_from(height).map_err(|_| RfvpError::CapacityExceeded)?,
        )
    }

    fn draw_sprite(&mut self, command: &DrawSpriteCommand) -> RfvpResult<()> {
        self.draw_sprite_direct(command)
    }

    fn draw_solid(&mut self, command: &DrawSolidCommand) -> RfvpResult<()> {
        self.ensure_white()?;
        let x0 = command.rect.x as f32;
        let y0 = command.rect.y as f32;
        let x1 = x0 + command.rect.width as f32;
        let y1 = y0 + command.rect.height as f32;
        let vertices = [
            vertex2d(x0, y1, 0.0, 1.0, command.color),
            vertex2d(x0, y0, 0.0, 0.0, command.color),
            vertex2d(x1, y1, 1.0, 1.0, command.color),
            vertex2d(x1, y0, 1.0, 0.0, command.color),
        ];
        self.draw_sprite_direct(&DrawSpriteCommand {
            texture: TextureId(u32::MAX),
            vertices: vertices.map(|vertex| vertex),
            blend: command.blend,
            filter: crate::host_api::TextureFilter::Nearest,
            scissor: command.scissor,
        })
    }

    fn end_frame(&mut self) -> RfvpResult<()> {
        self.backend.end_frame()
    }

    fn present(&mut self) -> RfvpResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct HostAudio {
    commands: Vec<AudioCommand>,
}

impl HostAudio {
    fn drain_commands(&mut self, out: &mut Vec<AudioCommand>) {
        out.extend(self.commands.drain(..));
    }
}

impl RfvpAudio for HostAudio {
    fn load_encoded(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: &[u8],
    ) -> RfvpResult<()> {
        self.commands.push(AudioCommand::LoadEncoded {
            id,
            kind,
            bytes: bytes.to_vec(),
        });
        Ok(())
    }

    fn create_stream(&mut self, id: AudioStreamId, desc: AudioStreamDesc) -> RfvpResult<()> {
        self.commands.push(AudioCommand::CreateStream { id, desc });
        Ok(())
    }

    fn submit_i16(&mut self, id: AudioStreamId, samples: &[i16]) -> RfvpResult<()> {
        self.commands.push(AudioCommand::SubmitI16 {
            id,
            samples: samples.to_vec(),
        });
        Ok(())
    }

    fn submit_f32(&mut self, id: AudioStreamId, samples: &[f32]) -> RfvpResult<()> {
        self.commands.push(AudioCommand::SubmitF32 {
            id,
            samples: samples.to_vec(),
        });
        Ok(())
    }

    fn play(&mut self, id: AudioStreamId, params: AudioParams, fade_in_ms: u32) -> RfvpResult<()> {
        self.commands.push(AudioCommand::Play {
            id,
            params,
            fade_in_ms,
        });
        Ok(())
    }

    fn stop(&mut self, id: AudioStreamId, fade_ms: u32) -> RfvpResult<()> {
        self.commands.push(AudioCommand::Stop { id, fade_ms });
        Ok(())
    }

    fn pause(&mut self, id: AudioStreamId) -> RfvpResult<()> {
        self.commands.push(AudioCommand::Pause { id });
        Ok(())
    }

    fn resume(&mut self, id: AudioStreamId) -> RfvpResult<()> {
        self.commands.push(AudioCommand::Resume { id });
        Ok(())
    }

    fn set_params(&mut self, id: AudioStreamId, params: AudioParams) -> RfvpResult<()> {
        self.commands.push(AudioCommand::SetParams { id, params });
        Ok(())
    }

    fn set_master_volume(&mut self, volume: f32) -> RfvpResult<()> {
        self.commands.push(AudioCommand::MasterVolume { volume });
        Ok(())
    }

    fn destroy_stream(&mut self, id: AudioStreamId) {
        self.commands.push(AudioCommand::DestroyStream { id });
    }

    fn tick(&mut self, _delta_us: u64) -> RfvpResult<()> {
        Ok(())
    }
}

struct HostClock {
    now_us: u64,
}

impl HostClock {
    fn new() -> Self {
        Self { now_us: 0 }
    }

    fn advance_ms(&mut self, delta_ms: u32) {
        self.now_us = self
            .now_us
            .saturating_add(u64::from(delta_ms).saturating_mul(1_000));
    }
}

impl RfvpClock for HostClock {
    fn ticks_us(&mut self) -> u64 {
        self.now_us
    }
}

struct HostFileSystem {
    root: PathBuf,
}

impl HostFileSystem {
    fn new(root: &str) -> Self {
        Self {
            root: PathBuf::from(root),
        }
    }

    fn resolve(&self, path: &str) -> RfvpResult<PathBuf> {
        let path = Path::new(path);
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root)
                .map_err(|_| RfvpError::InvalidArgument)?
        } else {
            path
        };
        if relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(RfvpError::InvalidArgument);
        }
        Ok(self.root.join(relative))
    }
}

struct HostFile {
    file: fs::File,
}

impl RfvpFile for HostFile {
    fn len(&mut self) -> RfvpResult<u64> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|_| RfvpError::Io)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RfvpResult<usize> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| RfvpError::Io)?;
        self.file.read(buf).map_err(|_| RfvpError::Io)
    }
}

impl RfvpFileSystem for HostFileSystem {
    type File = HostFile;

    fn open(&mut self, path: &str) -> RfvpResult<Self::File> {
        let path = self.resolve(path)?;
        fs::File::open(path)
            .map(|file| HostFile { file })
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    RfvpError::NotFound
                } else {
                    RfvpError::Io
                }
            })
    }

    fn write_all(&mut self, path: &str, bytes: &[u8]) -> RfvpResult<()> {
        let path = self.resolve(path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| RfvpError::Io)?;
        }
        fs::write(path, bytes).map_err(|_| RfvpError::Io)
    }

    fn metadata(&mut self, path: &str) -> RfvpResult<RfvpFileInfo> {
        let path = self.resolve(path)?;
        let metadata = fs::metadata(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RfvpError::NotFound
            } else {
                RfvpError::Io
            }
        })?;
        Ok(RfvpFileInfo {
            len: metadata.len(),
            kind: if metadata.is_file() {
                RfvpFileKind::File
            } else if metadata.is_dir() {
                RfvpFileKind::Directory
            } else {
                RfvpFileKind::Other
            },
        })
    }

    fn enumerate_by_extension(
        &mut self,
        root: &str,
        extension_without_dot: &str,
        visitor: &mut dyn FnMut(&str, RfvpFileInfo) -> RfvpResult<()>,
    ) -> RfvpResult<()> {
        let root = self.resolve(root)?;
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).map_err(|_| RfvpError::Io)? {
                let entry = entry.map_err(|_| RfvpError::Io)?;
                let path = entry.path();
                let metadata = entry.metadata().map_err(|_| RfvpError::Io)?;
                if metadata.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !metadata.is_file()
                    || path
                        .extension()
                        .and_then(|value| value.to_str())
                        .map(|value| value.eq_ignore_ascii_case(extension_without_dot))
                        != Some(true)
                {
                    continue;
                }
                let relative = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                visitor(&relative, RfvpFileInfo::file(metadata.len()))?;
            }
        }
        Ok(())
    }
}

fn vertex2d(x: f32, y: f32, u: f32, v: f32, color: ColorRgba) -> Vertex2D {
    Vertex2D {
        position: [x, y],
        tex_coord: [u, v],
        color,
    }
}

fn rgba8(color: ColorRgba) -> Rgba8 {
    Rgba8 {
        r: (color.r.clamp(0.0, 1.0) * 255.0) as u8,
        g: (color.g.clamp(0.0, 1.0) * 255.0) as u8,
        b: (color.b.clamp(0.0, 1.0) * 255.0) as u8,
        a: (color.a.clamp(0.0, 1.0) * 255.0) as u8,
    }
}

fn command_blend(blend: BlendMode) -> CommandBlendMode {
    match blend {
        BlendMode::Opaque | BlendMode::Alpha => CommandBlendMode::Normal,
        BlendMode::Add => CommandBlendMode::Add,
        BlendMode::Multiply => CommandBlendMode::Mul,
        BlendMode::Screen => CommandBlendMode::Add,
    }
}

fn with_state<R>(f: impl FnOnce(&mut HostState) -> R) -> R {
    HOST_STATE.with(|state| f(&mut state.borrow_mut()))
}

fn guard_status(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        log::error!("panic crossed the RFVP host ABI boundary");
        RFVP_STATUS_ENGINE
    })
}

fn guard_i32(f: impl FnOnce() -> i32) -> i32 {
    guard_status(f)
}

fn guard_u64(f: impl FnOnce() -> u64) -> u64 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        log::error!("panic crossed the RFVP host ABI boundary");
        0
    })
}

fn guard_void(f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        log::error!("panic crossed the RFVP host ABI boundary");
    }
}

fn read_utf8(ptr: *const u8, len: usize) -> Result<String, i32> {
    if ptr.is_null() {
        return if len == 0 {
            Ok(String::new())
        } else {
            Err(RFVP_STATUS_INVALID_ARGUMENT)
        };
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| RFVP_STATUS_INVALID_DATA)
}

fn nls_from_abi(value: u32) -> Result<Nls, i32> {
    match value {
        RFVP_NLS_SHIFT_JIS => Ok(Nls::ShiftJIS),
        RFVP_NLS_GBK => Ok(Nls::GBK),
        RFVP_NLS_UTF8 => Ok(Nls::UTF8),
        _ => Err(RFVP_STATUS_INVALID_ARGUMENT),
    }
}

fn color(rgba: Rgba8) -> RfvpColorV1 {
    RfvpColorV1 {
        r: rgba.r as f32 / 255.0,
        g: rgba.g as f32 / 255.0,
        b: rgba.b as f32 / 255.0,
        a: rgba.a as f32 / 255.0,
    }
}

fn abi_vertex(vertex: Vertex2D) -> RfvpVertexV1 {
    RfvpVertexV1 {
        x: vertex.position[0],
        y: vertex.position[1],
        u: vertex.tex_coord[0],
        v: vertex.tex_coord[1],
        color: RfvpColorV1 {
            r: vertex.color.r,
            g: vertex.color.g,
            b: vertex.color.b,
            a: vertex.color.a,
        },
    }
}

fn rect_i32(rect: RectI16) -> RfvpRectI32V1 {
    RfvpRectI32V1 {
        x: rect.x as i32,
        y: rect.y as i32,
        width: rect.w as i32,
        height: rect.h as i32,
    }
}

fn rect_u16(rect: RectU16) -> RfvpRectU16V1 {
    RfvpRectU16V1 {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
    }
}

fn blend(mode: CommandBlendMode) -> u32 {
    match mode {
        CommandBlendMode::Normal => RFVP_BLEND_NORMAL,
        CommandBlendMode::Add => RFVP_BLEND_ADD,
        CommandBlendMode::Sub => RFVP_BLEND_REVERSE_SUBTRACT,
        CommandBlendMode::Mul => RFVP_BLEND_MULTIPLY,
    }
}

fn empty_draw_command() -> RfvpDrawCommandV1 {
    RfvpDrawCommandV1 {
        struct_size: size_of::<RfvpDrawCommandV1>() as u32,
        kind: RFVP_DRAW_IMAGE,
        flags: 0,
        texture_id: 0,
        blend: RFVP_BLEND_NORMAL,
        filter: RFVP_TEXTURE_FILTER_LINEAR,
        effect_id: 0,
        mesh_topology: RFVP_MESH_TRIANGLE_LIST,
        src_rect: RfvpRectU16V1 {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        dst_rect: RfvpRectI32V1 {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        clip_rect: RfvpRectI32V1 {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        color: RfvpColorV1 {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        vertices: [RfvpVertexV1 {
            x: 0.0,
            y: 0.0,
            u: 0.0,
            v: 0.0,
            color: RfvpColorV1 {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
        }; 4],
        mesh: std::ptr::null(),
        mesh_vertex_count: 0,
        effect_data: std::ptr::null(),
        effect_data_size: 0,
        reserved: [0; 2],
    }
}

fn draw_image(command: &DrawImageCmd, clip: Option<RectI16>) -> RfvpDrawCommandV1 {
    let mut output = empty_draw_command();
    output.kind = RFVP_DRAW_IMAGE;
    output.texture_id = command.texture.0;
    output.blend = blend(command.blend);
    output.effect_id = command.effect_id as u32;
    output.src_rect = rect_u16(command.src);
    output.dst_rect = rect_i32(command.dst);
    output.color = color(command.color);
    output.vertices = command.vertices.map(abi_vertex);
    if command.src.w > 0 && command.src.h > 0 {
        output.flags |= RFVP_DRAW_FLAG_HAS_SRC_RECT;
    }
    if let Some(clip) = command.clip.or(clip) {
        output.clip_rect = rect_i32(clip);
        output.flags |= RFVP_DRAW_FLAG_HAS_CLIP;
    }
    output
}

fn draw_glyph(command: &DrawGlyphCmd, clip: Option<RectI16>) -> RfvpDrawCommandV1 {
    let mut output = empty_draw_command();
    output.kind = RFVP_DRAW_GLYPH;
    output.texture_id = command.texture.0;
    output.src_rect = rect_u16(command.src);
    output.dst_rect = rect_i32(command.dst);
    output.color = color(command.color);
    let x0 = command.dst.x as f32;
    let y0 = command.dst.y as f32;
    let x1 = command.dst.x.saturating_add(command.dst.w) as f32;
    let y1 = command.dst.y.saturating_add(command.dst.h) as f32;
    let color = color(command.color);
    output.vertices = [
        RfvpVertexV1 {
            x: x0,
            y: y1,
            u: 0.0,
            v: 1.0,
            color,
        },
        RfvpVertexV1 {
            x: x0,
            y: y0,
            u: 0.0,
            v: 0.0,
            color,
        },
        RfvpVertexV1 {
            x: x1,
            y: y1,
            u: 1.0,
            v: 1.0,
            color,
        },
        RfvpVertexV1 {
            x: x1,
            y: y0,
            u: 1.0,
            v: 0.0,
            color,
        },
    ];
    if command.src.w > 0 && command.src.h > 0 {
        output.flags |= RFVP_DRAW_FLAG_HAS_SRC_RECT;
    }
    if let Some(clip) = command.clip.or(clip) {
        output.clip_rect = rect_i32(clip);
        output.flags |= RFVP_DRAW_FLAG_HAS_CLIP;
    }
    output
}

fn texture_format(format: TextureFormat) -> Option<u32> {
    match format {
        TextureFormat::Rgba8 => Some(RFVP_TEXTURE_FORMAT_RGBA8),
        TextureFormat::LumaA8 => Some(RFVP_TEXTURE_FORMAT_LUMA_A8),
        _ => None,
    }
}

fn texture_pixel_bytes(format: TextureFormat) -> u32 {
    match format {
        TextureFormat::Rgba8 => 4,
        TextureFormat::LumaA8 => 2,
        _ => 0,
    }
}

impl HostFrame {
    fn new(external: ExternalFrame, width: u32, height: u32, runtime: u64) -> Self {
        let mut texture_pixels = Vec::new();
        let mut textures = Vec::new();
        for command in &external.texture_commands {
            let pixels = match command {
                RecordedTextureCommand::Create(texture) => texture.pixels.clone(),
                RecordedTextureCommand::Update(update) => update.pixels.clone(),
                RecordedTextureCommand::Destroy(_) => Vec::new(),
            };
            let pixels_ptr = pixels.as_ptr();
            let pixels_size = pixels.len();
            let command = match command {
                RecordedTextureCommand::Create(texture) => {
                    let Some(format) = texture_format(texture.desc.format) else {
                        continue;
                    };
                    RfvpTextureCommandV1 {
                        struct_size: size_of::<RfvpTextureCommandV1>() as u32,
                        kind: RFVP_TEXTURE_CREATE,
                        texture_id: texture.handle.0,
                        format,
                        width: texture.desc.width as u32,
                        height: texture.desc.height as u32,
                        mip_count: 1,
                        row_bytes: texture.desc.width as u32
                            * texture_pixel_bytes(texture.desc.format),
                        rect: RfvpRectI32V1 {
                            x: 0,
                            y: 0,
                            width: texture.desc.width as i32,
                            height: texture.desc.height as i32,
                        },
                        generation: texture.generation,
                        pixels: pixels_ptr,
                        pixels_size,
                        reserved: [0; 2],
                    }
                }
                RecordedTextureCommand::Update(update) => RfvpTextureCommandV1 {
                    struct_size: size_of::<RfvpTextureCommandV1>() as u32,
                    kind: crate::host_abi::v1::RFVP_TEXTURE_UPDATE,
                    texture_id: update.handle.0,
                    format: texture_format(update.format).unwrap_or_default(),
                    width: update.rect.width,
                    height: update.rect.height,
                    mip_count: 1,
                    row_bytes: update.rect.width * texture_pixel_bytes(update.format),
                    rect: RfvpRectI32V1 {
                        x: update.rect.x as i32,
                        y: update.rect.y as i32,
                        width: update.rect.width as i32,
                        height: update.rect.height as i32,
                    },
                    generation: update.generation,
                    pixels: pixels_ptr,
                    pixels_size,
                    reserved: [0; 2],
                },
                RecordedTextureCommand::Destroy(destroy) => RfvpTextureCommandV1 {
                    struct_size: size_of::<RfvpTextureCommandV1>() as u32,
                    kind: crate::host_abi::v1::RFVP_TEXTURE_DESTROY,
                    texture_id: destroy.handle.0,
                    format: 0,
                    width: 0,
                    height: 0,
                    mip_count: 0,
                    row_bytes: 0,
                    rect: RfvpRectI32V1 {
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    },
                    generation: 0,
                    pixels: std::ptr::null(),
                    pixels_size: 0,
                    reserved: [0; 2],
                },
            };
            texture_pixels.push(pixels);
            textures.push(command);
        }

        let mut commands = Vec::new();
        let mut active_clip = None;
        for command in &external.frame.commands {
            match command {
                RenderCommand::SetClip(rect) => active_clip = Some(*rect),
                RenderCommand::ClearClip => active_clip = None,
                RenderCommand::DrawImage(command) => {
                    commands.push(draw_image(command, active_clip));
                }
                RenderCommand::DrawGlyph(command) => {
                    commands.push(draw_glyph(command, active_clip));
                }
            }
        }

        let hit_proxies = external
            .frame
            .hit_proxies
            .proxies
            .iter()
            .map(|proxy| {
                let mut flags = 0;
                if proxy.enabled {
                    flags |= crate::host_abi::v1::RFVP_HIT_PROXY_ENABLED;
                }
                if proxy.visible {
                    flags |= crate::host_abi::v1::RFVP_HIT_PROXY_VISIBLE;
                }
                RfvpHitProxyV1 {
                    prim_id: proxy.prim_id.0,
                    flags,
                    rect: rect_i32(proxy.rect),
                    order: proxy.order,
                    reserved0: 0,
                }
            })
            .collect();

        Self {
            width,
            height,
            runtime,
            commands,
            textures,
            hit_proxies,
            _texture_pixels: texture_pixels,
        }
    }
}

fn encoded_audio_kind(kind: EncodedAudioKind) -> u32 {
    match kind {
        EncodedAudioKind::Unknown => RFVP_AUDIO_ENCODED_UNKNOWN,
        EncodedAudioKind::Wav => RFVP_AUDIO_ENCODED_WAV,
        EncodedAudioKind::Ogg => RFVP_AUDIO_ENCODED_OGG,
        EncodedAudioKind::Mp3 => RFVP_AUDIO_ENCODED_MP3,
        EncodedAudioKind::Flac => RFVP_AUDIO_ENCODED_FLAC,
    }
}

fn audio_sample_format(format: AudioSampleFormat) -> u32 {
    match format {
        AudioSampleFormat::I16 => RFVP_AUDIO_SAMPLE_I16,
        AudioSampleFormat::F32 => RFVP_AUDIO_SAMPLE_F32,
    }
}

fn empty_audio_command(kind: u32, stream_id: u32) -> RfvpAudioCommandV1 {
    RfvpAudioCommandV1 {
        struct_size: size_of::<RfvpAudioCommandV1>() as u32,
        kind,
        stream_id,
        sample_format: 0,
        encoded_kind: 0,
        sample_rate: 0,
        channels: 0,
        repeat: 0,
        fade_ms: 0,
        volume: 1.0,
        pan: 0.0,
        sample_count: 0,
        payload: std::ptr::null(),
        payload_size: 0,
        reserved: [0; 2],
    }
}

fn pending_audio_command(command: AudioCommand) -> PendingAudioCommand {
    let mut payload = Vec::new();
    let mut output = empty_audio_command(0, 0);
    match command {
        AudioCommand::LoadEncoded { id, kind, bytes } => {
            output.kind = RFVP_AUDIO_LOAD_ENCODED;
            output.stream_id = id.0;
            output.encoded_kind = encoded_audio_kind(kind);
            payload = bytes;
        }
        AudioCommand::CreateStream { id, desc } => {
            output.kind = RFVP_AUDIO_CREATE_STREAM;
            output.stream_id = id.0;
            output.sample_format = audio_sample_format(desc.sample_format);
            output.sample_rate = desc.sample_rate;
            output.channels = desc.channels as u32;
        }
        AudioCommand::SubmitI16 { id, samples } => {
            output.kind = RFVP_AUDIO_SUBMIT_I16;
            output.stream_id = id.0;
            output.sample_format = RFVP_AUDIO_SAMPLE_I16;
            output.sample_count = samples.len();
            payload.reserve(samples.len().saturating_mul(2));
            for sample in samples {
                payload.extend_from_slice(&sample.to_le_bytes());
            }
        }
        AudioCommand::SubmitF32 { id, samples } => {
            output.kind = RFVP_AUDIO_SUBMIT_F32;
            output.stream_id = id.0;
            output.sample_format = RFVP_AUDIO_SAMPLE_F32;
            output.sample_count = samples.len();
            payload.reserve(samples.len().saturating_mul(4));
            for sample in samples {
                payload.extend_from_slice(&sample.to_le_bytes());
            }
        }
        AudioCommand::Play {
            id,
            params,
            fade_in_ms,
        } => {
            output.kind = RFVP_AUDIO_PLAY;
            output.stream_id = id.0;
            output.volume = params.volume;
            output.pan = params.pan;
            output.repeat = u32::from(params.repeat);
            output.fade_ms = fade_in_ms;
        }
        AudioCommand::Stop { id, fade_ms } => {
            output.kind = RFVP_AUDIO_STOP;
            output.stream_id = id.0;
            output.fade_ms = fade_ms;
        }
        AudioCommand::Pause { id } => {
            output.kind = RFVP_AUDIO_PAUSE;
            output.stream_id = id.0;
        }
        AudioCommand::Resume { id } => {
            output.kind = RFVP_AUDIO_RESUME;
            output.stream_id = id.0;
        }
        AudioCommand::SetParams { id, params } => {
            output.kind = RFVP_AUDIO_SET_PARAMS;
            output.stream_id = id.0;
            output.volume = params.volume;
            output.pan = params.pan;
            output.repeat = u32::from(params.repeat);
        }
        AudioCommand::DestroyStream { id } => {
            output.kind = RFVP_AUDIO_DESTROY_STREAM;
            output.stream_id = id.0;
        }
        AudioCommand::MasterVolume { volume } => {
            output.kind = RFVP_AUDIO_MASTER_VOLUME;
            output.volume = volume;
        }
    }
    output.payload_size = payload.len();
    PendingAudioCommand {
        command: output,
        payload,
    }
}

fn pointer_button(code: u32) -> Option<PointerButton> {
    match code {
        RFVP_POINTER_LEFT => Some(PointerButton::Left),
        RFVP_POINTER_RIGHT => Some(PointerButton::Right),
        RFVP_POINTER_MIDDLE => Some(PointerButton::Middle),
        _ => None,
    }
}

fn key_code(code: u32) -> KeyCode {
    match code {
        RFVP_KEY_ESCAPE => KeyCode::Escape,
        RFVP_KEY_RETURN => KeyCode::Return,
        RFVP_KEY_SPACE => KeyCode::Space,
        RFVP_KEY_BACKSPACE => KeyCode::Backspace,
        RFVP_KEY_TAB => KeyCode::Tab,
        RFVP_KEY_LEFT => KeyCode::Left,
        RFVP_KEY_RIGHT => KeyCode::Right,
        RFVP_KEY_UP => KeyCode::Up,
        RFVP_KEY_DOWN => KeyCode::Down,
        RFVP_KEY_PAGE_UP => KeyCode::PageUp,
        RFVP_KEY_PAGE_DOWN => KeyCode::PageDown,
        RFVP_KEY_HOME => KeyCode::Home,
        RFVP_KEY_END => KeyCode::End,
        RFVP_KEY_INSERT => KeyCode::Insert,
        RFVP_KEY_DELETE => KeyCode::Delete,
        RFVP_KEY_SHIFT => KeyCode::Shift,
        RFVP_KEY_CONTROL => KeyCode::Control,
        RFVP_KEY_ALT => KeyCode::Alt,
        value if char::from_u32(value).is_some_and(|ch| !ch.is_control()) => {
            KeyCode::Character(char::from_u32(value).expect("validated unicode scalar"))
        }
        value => KeyCode::Unknown(value),
    }
}

fn input_event(event: &RfvpInputEventV1, virtual_size: (u32, u32)) -> Result<RfvpEvent, i32> {
    if (event.struct_size as usize) < size_of::<RfvpInputEventV1>() {
        return Err(RFVP_STATUS_INVALID_ARGUMENT);
    }
    let modifiers = InputModifiers::from_bits(event.modifiers);
    let event = match event.kind {
        RFVP_INPUT_KEY => match event.phase {
            RFVP_INPUT_PHASE_DOWN | RFVP_INPUT_PHASE_REPEAT => RfvpEvent::KeyDown {
                key: key_code(event.code),
                repeat: event.phase == RFVP_INPUT_PHASE_REPEAT,
                modifiers,
            },
            RFVP_INPUT_PHASE_UP => RfvpEvent::KeyUp {
                key: key_code(event.code),
                modifiers,
            },
            _ => return Err(RFVP_STATUS_INVALID_ARGUMENT),
        },
        RFVP_INPUT_TEXT => {
            let Some(ch) = char::from_u32(event.code) else {
                return Err(RFVP_STATUS_INVALID_ARGUMENT);
            };
            RfvpEvent::TextInput { ch }
        }
        RFVP_INPUT_POINTER_MOVE => RfvpEvent::PointerMove {
            x: event.x,
            y: event.y,
            in_screen: event.x >= 0
                && event.y >= 0
                && event.x < virtual_size.0 as i32
                && event.y < virtual_size.1 as i32,
        },
        RFVP_INPUT_POINTER_BUTTON => {
            let Some(button) = pointer_button(event.code) else {
                return Err(RFVP_STATUS_INVALID_ARGUMENT);
            };
            match event.phase {
                RFVP_INPUT_PHASE_DOWN => RfvpEvent::PointerDown {
                    button,
                    x: event.x,
                    y: event.y,
                },
                RFVP_INPUT_PHASE_UP => RfvpEvent::PointerUp {
                    button,
                    x: event.x,
                    y: event.y,
                },
                _ => return Err(RFVP_STATUS_INVALID_ARGUMENT),
            }
        }
        RFVP_INPUT_WHEEL => RfvpEvent::Wheel {
            delta_x: event.x,
            delta_y: event.y,
        },
        RFVP_INPUT_TOUCH => match event.phase {
            RFVP_INPUT_PHASE_DOWN => RfvpEvent::TouchDown {
                id: event.id,
                x: event.x,
                y: event.y,
            },
            RFVP_INPUT_PHASE_MOVE => RfvpEvent::TouchMove {
                id: event.id,
                x: event.x,
                y: event.y,
            },
            RFVP_INPUT_PHASE_UP => RfvpEvent::TouchUp {
                id: event.id,
                x: event.x,
                y: event.y,
            },
            _ => return Err(RFVP_STATUS_INVALID_ARGUMENT),
        },
        RFVP_INPUT_FOCUS => {
            if event.phase == 0 {
                RfvpEvent::FocusLost
            } else {
                RfvpEvent::FocusGained
            }
        }
        RFVP_INPUT_QUIT => RfvpEvent::Quit,
        _ => return Err(RFVP_STATUS_INVALID_ARGUMENT),
    };
    Ok(event)
}

pub unsafe extern "C" fn rfvp_resources_create(
    config: *const RfvpResourcesConfigV1,
    out_resources: *mut u64,
) -> i32 {
    guard_status(|| {
        if config.is_null() || out_resources.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let config = unsafe { &*config };
        if (config.struct_size as usize) < size_of::<RfvpResourcesConfigV1>() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let nls = match nls_from_abi(config.nls) {
            Ok(nls) => nls,
            Err(status) => return status,
        };
        let save_root = match read_utf8(config.save_root_utf8, config.save_root_len) {
            Ok(value) if value.is_empty() => None,
            Ok(value) => Some(value),
            Err(status) => return status,
        };

        let handle = with_state(|state| {
            state.resources.insert(HostResources {
                nls,
                game_root: None,
                save_root,
            })
        });
        unsafe { *out_resources = handle.raw() };
        RFVP_STATUS_OK
    })
}

pub unsafe extern "C" fn rfvp_resources_destroy(resources: u64) {
    guard_void(|| {
        with_state(|state| {
            state.resources.remove(Handle::from_raw(resources));
        });
    });
}

pub unsafe extern "C" fn rfvp_resources_clear(resources: u64) {
    guard_void(|| {
        with_state(|state| {
            if let Some(resources) = state.resources.get_mut(Handle::from_raw(resources)) {
                resources.game_root = None;
            }
        });
    });
}

pub unsafe extern "C" fn rfvp_resources_mount_directory(
    resources: u64,
    path_utf8: *const u8,
    path_len: usize,
) -> i32 {
    guard_status(|| {
        let path = match read_utf8(path_utf8, path_len) {
            Ok(path) if !path.is_empty() => path,
            Ok(_) => return RFVP_STATUS_INVALID_ARGUMENT,
            Err(status) => return status,
        };
        let exists = with_state(|state| state.resources.get(Handle::from_raw(resources)).is_some());
        if !exists {
            return RFVP_STATUS_INVALID_HANDLE;
        }
        if !Path::new(&path).is_dir() {
            return RFVP_STATUS_NOT_FOUND;
        }
        let updated = with_state(|state| {
            let Some(resources) = state.resources.get_mut(Handle::from_raw(resources)) else {
                return false;
            };
            resources.game_root = Some(path);
            true
        });
        if updated {
            RFVP_STATUS_OK
        } else {
            RFVP_STATUS_INVALID_HANDLE
        }
    })
}

pub unsafe extern "C" fn rfvp_resources_mount_pack(
    _resources: u64,
    _folder_utf8: *const u8,
    _folder_len: usize,
    _pack_data: *const u8,
    _pack_size: usize,
) -> i32 {
    RFVP_STATUS_UNSUPPORTED
}

pub unsafe extern "C" fn rfvp_resources_set_override(
    _resources: u64,
    _path_utf8: *const u8,
    _path_len: usize,
    _data: *const u8,
    _data_size: usize,
) -> i32 {
    RFVP_STATUS_UNSUPPORTED
}

pub unsafe extern "C" fn rfvp_resources_clear_overrides(_resources: u64) {}

pub unsafe extern "C" fn rfvp_resources_set_save_root(
    resources: u64,
    path_utf8: *const u8,
    path_len: usize,
) -> i32 {
    guard_status(|| {
        let path = match read_utf8(path_utf8, path_len) {
            Ok(path) if !path.is_empty() => Some(path),
            Ok(_) => None,
            Err(status) => return status,
        };
        let updated = with_state(|state| {
            let Some(resources) = state.resources.get_mut(Handle::from_raw(resources)) else {
                return false;
            };
            resources.save_root = path;
            true
        });
        if updated {
            RFVP_STATUS_OK
        } else {
            RFVP_STATUS_INVALID_HANDLE
        }
    })
}

pub unsafe extern "C" fn rfvp_runtime_create(
    config: *const RfvpRuntimeConfigV1,
    out_runtime: *mut u64,
) -> i32 {
    guard_status(|| {
        if config.is_null() || out_runtime.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let config = unsafe { &*config };
        if (config.struct_size as usize) < size_of::<RfvpRuntimeConfigV1>() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        if config.resources == RFVP_INVALID_HANDLE {
            return RFVP_STATUS_INVALID_HANDLE;
        }

        let resources = with_state(|state| {
            let Some(resources) = state.resources.get(Handle::from_raw(config.resources)) else {
                return None;
            };
            Some((resources.game_root.clone(), resources.nls))
        });
        let Some((game_root, nls)) = resources else {
            return RFVP_STATUS_INVALID_HANDLE;
        };

        let Some(game_root) = game_root else {
            return RFVP_STATUS_INVALID_DATA;
        };

        let mut core = RfvpCore::new(RfvpCoreConfig::default());
        let mut host = HostPlatform::new(&game_root);
        let boot = RfvpBootConfig {
            asset_root: &game_root,
            hcb_extension: "hcb",
            max_hcb_bytes: 128 * 1024 * 1024,
            max_manifest_entries: 16 * 1024,
            nls,
            require_default_font: false,
        };
        if let Err(error) = core.boot(&mut host, boot) {
            let detail = core.last_error_detail().unwrap_or("no detail");
            log::error!("rfvp_runtime_create failed: {error:?}: {detail}");
            return RFVP_STATUS_ENGINE;
        }
        let (width, height) = (core.config().virtual_width, core.config().virtual_height);
        let handle = with_state(|state| {
            state.runtimes.insert(HostRuntime {
                core,
                host,
                pending_frame: None,
                audio_commands: VecDeque::new(),
                pending_audio_command: None,
                audio_queue_overflowed: false,
                width,
                height,
                exit_requested: false,
                active_frame: None,
            })
        });
        unsafe { *out_runtime = handle.raw() };
        RFVP_STATUS_OK
    })
}

pub unsafe extern "C" fn rfvp_runtime_destroy(runtime: u64) {
    guard_void(|| {
        with_state(|state| {
            state.runtimes.remove(Handle::from_raw(runtime));
        });
    });
}

pub unsafe extern "C" fn rfvp_runtime_step(runtime: u64, delta_ms: u32) -> i32 {
    guard_status(|| {
        with_state(|state| {
            let Some(runtime) = state.runtimes.get_mut(Handle::from_raw(runtime)) else {
                return RFVP_STATUS_INVALID_HANDLE;
            };
            let delta_ms = delta_ms.max(1);
            runtime.host.clock.advance_ms(delta_ms);
            if let Err(error) = runtime.core.tick(&mut runtime.host) {
                log::error!("rfvp_runtime_step failed: {error:?}");
                return RFVP_STATUS_ENGINE;
            }
            runtime.exit_requested |= runtime.core.exit_requested();
            let mut audio_commands = Vec::new();
            runtime.host.audio.drain_commands(&mut audio_commands);
            for command in audio_commands {
                if runtime.audio_commands.len() >= MAX_PENDING_AUDIO_COMMANDS {
                    if !runtime.audio_queue_overflowed {
                        runtime.audio_queue_overflowed = true;
                        log::warn!("RFVP host ABI audio queue is full; dropping commands");
                    }
                    break;
                }
                runtime.audio_commands.push_back(command);
            }
            let mut next_frame = runtime.host.renderer.take_external_frame();
            if let Some(mut pending) = runtime.pending_frame.take() {
                // Logic-only catch-up ticks may advance several frames before
                // the host presents. Preserve texture create/update/destroy
                // commands from every skipped frame so the next presented
                // frame still references a valid texture state.
                pending.texture_commands.append(&mut next_frame.texture_commands);
                next_frame.texture_commands = pending.texture_commands;
            }
            runtime.pending_frame = Some(next_frame);
            runtime.width = runtime.core.config().virtual_width;
            runtime.height = runtime.core.config().virtual_height;
            RFVP_STATUS_OK
        })
    })
}

pub unsafe extern "C" fn rfvp_runtime_is_exit_requested(runtime: u64) -> i32 {
    guard_i32(|| {
        with_state(|state| {
            state
                .runtimes
                .get(Handle::from_raw(runtime))
                .map(|runtime| if runtime.exit_requested { 1 } else { 0 })
                .unwrap_or(0)
        })
    })
}

pub unsafe extern "C" fn rfvp_runtime_push_input(
    runtime: u64,
    events: *const RfvpInputEventV1,
    event_count: usize,
) -> i32 {
    guard_status(|| {
        if events.is_null() && event_count != 0 {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        if event_count > 4096 {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        with_state(|state| {
            let Some(runtime) = state.runtimes.get_mut(Handle::from_raw(runtime)) else {
                return RFVP_STATUS_INVALID_HANDLE;
            };
            let virtual_size = (
                runtime.core.config().virtual_width,
                runtime.core.config().virtual_height,
            );
            for index in 0..event_count {
                let event = unsafe { &*events.add(index) };
                let event = match input_event(event, virtual_size) {
                    Ok(event) => event,
                    Err(status) => return status,
                };
                if let Err(error) = runtime.core.push_event(event) {
                    log::warn!("rfvp_runtime_push_input rejected event: {error:?}");
                    return RFVP_STATUS_BUSY;
                }
            }
            RFVP_STATUS_OK
        })
    })
}

pub unsafe extern "C" fn rfvp_runtime_poll_audio_command(
    runtime: u64,
    out_command: *mut RfvpAudioCommandV1,
) -> i32 {
    guard_status(|| {
        if out_command.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        with_state(|state| {
            let Some(runtime) = state.runtimes.get_mut(Handle::from_raw(runtime)) else {
                return RFVP_STATUS_INVALID_HANDLE;
            };
            let Some(command) = runtime.audio_commands.pop_front() else {
                return RFVP_STATUS_NO_COMMAND;
            };
            let mut pending = pending_audio_command(command);
            pending.command.payload = pending.payload.as_ptr();
            runtime.pending_audio_command = Some(pending);
            let Some(pending) = runtime.pending_audio_command.as_ref() else {
                return RFVP_STATUS_ENGINE;
            };
            unsafe {
                *out_command = pending.command;
            }
            RFVP_STATUS_OK
        })
    })
}

pub unsafe extern "C" fn rfvp_runtime_capabilities(runtime: u64) -> u64 {
    guard_u64(|| {
        with_state(|state| {
            state
                .runtimes
                .get(Handle::from_raw(runtime))
                .map(|_| {
                    RFVP_CAPABILITY_TEXTURES
                        | RFVP_CAPABILITY_DRAW_IMAGE
                        | RFVP_CAPABILITY_DRAW_GLYPH
                        | RFVP_CAPABILITY_HIT_PROXIES
                        | RFVP_CAPABILITY_AUDIO_COMMANDS
                })
                .unwrap_or(0)
        })
    })
}

pub unsafe extern "C" fn rfvp_runtime_acquire_frame(runtime: u64, out_frame: *mut u64) -> i32 {
    guard_status(|| {
        if out_frame.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }

        let result = with_state(|state| {
            let Some(runtime) = state.runtimes.get_mut(Handle::from_raw(runtime)) else {
                return None;
            };
            if runtime.active_frame.is_some() {
                return Some(Err(RFVP_STATUS_BUSY));
            }
            let Some(external) = runtime.pending_frame.take() else {
                return Some(Err(RFVP_STATUS_NO_FRAME));
            };
            Some(Ok((external, runtime.width, runtime.height)))
        });

        let Some(result) = result else {
            return RFVP_STATUS_INVALID_HANDLE;
        };
        let (external, width, height) = match result {
            Ok(value) => value,
            Err(status) => return status,
        };
        let frame = HostFrame::new(external, width, height, runtime);
        let handle = with_state(|state| state.frames.insert(frame));
        let frame_raw = handle.raw();
        with_state(|state| {
            if let Some(runtime) = state.runtimes.get_mut(Handle::from_raw(runtime)) {
                runtime.active_frame = Some(frame_raw);
            }
        });
        unsafe { *out_frame = frame_raw };
        RFVP_STATUS_OK
    })
}

pub unsafe extern "C" fn rfvp_frame_release(frame: u64) {
    guard_void(|| {
        let released_raw = frame;
        let removed = with_state(|state| state.frames.remove(Handle::from_raw(released_raw)));
        if let Some(frame) = removed {
            with_state(|state| {
                if let Some(runtime) = state.runtimes.get_mut(Handle::from_raw(frame.runtime)) {
                    if runtime.active_frame == Some(released_raw) {
                        runtime.active_frame = None;
                    }
                }
            });
        }
    });
}

pub unsafe extern "C" fn rfvp_frame_get_size(
    frame: u64,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32 {
    guard_status(|| {
        if out_width.is_null() || out_height.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let size = with_state(|state| {
            state
                .frames
                .get(Handle::from_raw(frame))
                .map(|frame| (frame.width, frame.height))
        });
        let Some((width, height)) = size else {
            return RFVP_STATUS_INVALID_HANDLE;
        };
        unsafe {
            *out_width = width;
            *out_height = height;
        }
        RFVP_STATUS_OK
    })
}

pub unsafe extern "C" fn rfvp_frame_get_commands(
    frame: u64,
    out_commands: *mut *const RfvpDrawCommandV1,
    out_count: *mut usize,
) -> i32 {
    guard_status(|| {
        if out_commands.is_null() || out_count.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let result = with_state(|state| {
            state
                .frames
                .get(Handle::from_raw(frame))
                .map(|frame| (frame.commands.as_ptr(), frame.commands.len()))
        });
        let Some((commands, count)) = result else {
            return RFVP_STATUS_INVALID_HANDLE;
        };
        unsafe {
            *out_commands = commands;
            *out_count = count;
        }
        RFVP_STATUS_OK
    })
}

pub unsafe extern "C" fn rfvp_frame_get_textures(
    frame: u64,
    out_commands: *mut *const RfvpTextureCommandV1,
    out_count: *mut usize,
) -> i32 {
    guard_status(|| {
        if out_commands.is_null() || out_count.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let result = with_state(|state| {
            state
                .frames
                .get(Handle::from_raw(frame))
                .map(|frame| (frame.textures.as_ptr(), frame.textures.len()))
        });
        let Some((commands, count)) = result else {
            return RFVP_STATUS_INVALID_HANDLE;
        };
        unsafe {
            *out_commands = commands;
            *out_count = count;
        }
        RFVP_STATUS_OK
    })
}

pub unsafe extern "C" fn rfvp_frame_get_hit_proxies(
    frame: u64,
    out_proxies: *mut *const RfvpHitProxyV1,
    out_count: *mut usize,
) -> i32 {
    guard_status(|| {
        if out_proxies.is_null() || out_count.is_null() {
            return RFVP_STATUS_INVALID_ARGUMENT;
        }
        let result = with_state(|state| {
            state
                .frames
                .get(Handle::from_raw(frame))
                .map(|frame| (frame.hit_proxies.as_ptr(), frame.hit_proxies.len()))
        });
        let Some((proxies, count)) = result else {
            return RFVP_STATUS_INVALID_HANDLE;
        };
        unsafe {
            *out_proxies = proxies;
            *out_count = count;
        }
        RFVP_STATUS_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    fn resources_config() -> RfvpResourcesConfigV1 {
        RfvpResourcesConfigV1 {
            struct_size: size_of::<RfvpResourcesConfigV1>() as u32,
            flags: 0,
            nls: RFVP_NLS_SHIFT_JIS,
            reserved0: 0,
            save_root_utf8: ptr::null(),
            save_root_len: 0,
            reserved: [0; 4],
        }
    }

    #[test]
    fn resources_round_trip_and_missing_mount_is_reported() {
        let config = resources_config();
        let mut resources = 0u64;
        let status = unsafe { rfvp_resources_create(&config, &mut resources) };
        assert_eq!(status, RFVP_STATUS_OK);
        assert_ne!(resources, RFVP_INVALID_HANDLE);

        let path = b"/rfvp/does-not-exist";
        let status =
            unsafe { rfvp_resources_mount_directory(resources, path.as_ptr(), path.len()) };
        assert_eq!(status, RFVP_STATUS_NOT_FOUND);

        unsafe { rfvp_resources_destroy(resources) };
        let status =
            unsafe { rfvp_resources_mount_directory(resources, path.as_ptr(), path.len()) };
        assert_eq!(status, RFVP_STATUS_INVALID_HANDLE);
    }

    #[test]
    fn runtime_create_rejects_missing_resources() {
        let config = RfvpRuntimeConfigV1 {
            struct_size: size_of::<RfvpRuntimeConfigV1>() as u32,
            flags: 0,
            resources: 42,
            requested_width: 1280,
            requested_height: 720,
            reserved: [0; 4],
        };
        let mut runtime = 0u64;
        let status = unsafe { rfvp_runtime_create(&config, &mut runtime) };
        assert_eq!(status, RFVP_STATUS_INVALID_HANDLE);
        assert_eq!(runtime, 0);
    }

    #[test]
    fn audio_command_conversion_preserves_stream_and_payload() {
        let pending = pending_audio_command(AudioCommand::LoadEncoded {
            id: crate::host_api::AudioStreamId::bgm(3),
            kind: crate::host_api::EncodedAudioKind::Ogg,
            bytes: vec![1, 2, 3, 4],
        });

        assert_eq!(pending.command.kind, RFVP_AUDIO_LOAD_ENCODED);
        assert_eq!(pending.command.stream_id, 3);
        assert_eq!(pending.command.encoded_kind, RFVP_AUDIO_ENCODED_OGG);
        assert_eq!(pending.command.payload_size, 4);
        assert_eq!(pending.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn input_conversion_maps_keys_pointer_and_touch() {
        let key = RfvpInputEventV1 {
            struct_size: size_of::<RfvpInputEventV1>() as u32,
            kind: RFVP_INPUT_KEY,
            code: RFVP_KEY_RETURN,
            phase: RFVP_INPUT_PHASE_DOWN,
            x: 0,
            y: 0,
            value: 0,
            modifiers: 0,
            id: 0,
        };
        assert_eq!(
            input_event(&key, (1280, 720)),
            Ok(RfvpEvent::KeyDown {
                key: KeyCode::Return,
                repeat: false,
                modifiers: InputModifiers::empty(),
            })
        );

        let pointer = RfvpInputEventV1 {
            struct_size: size_of::<RfvpInputEventV1>() as u32,
            kind: RFVP_INPUT_POINTER_BUTTON,
            code: RFVP_POINTER_LEFT,
            phase: RFVP_INPUT_PHASE_DOWN,
            x: 12,
            y: 34,
            value: 0,
            modifiers: 0,
            id: 0,
        };
        assert_eq!(
            input_event(&pointer, (1280, 720)),
            Ok(RfvpEvent::PointerDown {
                button: PointerButton::Left,
                x: 12,
                y: 34,
            })
        );

        let touch = RfvpInputEventV1 {
            struct_size: size_of::<RfvpInputEventV1>() as u32,
            kind: RFVP_INPUT_TOUCH,
            code: 0,
            phase: RFVP_INPUT_PHASE_MOVE,
            x: 5,
            y: 6,
            value: 0,
            modifiers: 0,
            id: 7,
        };
        assert_eq!(
            input_event(&touch, (1280, 720)),
            Ok(RfvpEvent::TouchMove { id: 7, x: 5, y: 6 })
        );
    }
}
