//! Backend-neutral frame collection for an external render host.
//!
//! The production RFVP application still owns its wgpu surface. This module
//! exposes the already backend-neutral scene traversal and a recording
//! implementation that can be used by fixtures and host-side adapters without
//! enabling `gpu-render`.

use alloc::vec::Vec;

use crate::host_api::{
    PortableTextureDesc, RenderBackend, RenderCommand, RenderFrame, RfvpError, TextureBackend,
    TextureHandle, TextureRect,
};

pub use super::prim_commands::{render_motion_to_host, HostPrimRenderCache};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedTextureCreate {
    pub handle: TextureHandle,
    pub desc: PortableTextureDesc,
    pub pixels: Vec<u8>,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedTextureUpdate {
    pub handle: TextureHandle,
    pub rect: TextureRect,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedTextureDestroy {
    pub handle: TextureHandle,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingBackend {
    pub creates: Vec<RecordedTextureCreate>,
    pub updates: Vec<RecordedTextureUpdate>,
    pub destroys: Vec<RecordedTextureDestroy>,
    pub frame: RenderFrame,
    pub begin_frame_calls: usize,
    pub end_frame_calls: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalFrame {
    pub frame: RenderFrame,
    pub textures: Vec<RecordedTextureCreate>,
}

impl RecordingBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_frame(&mut self) -> RenderFrame {
        core::mem::take(&mut self.frame)
    }
}

impl TextureBackend for RecordingBackend {
    type Error = RfvpError;

    fn create_texture(
        &mut self,
        handle: TextureHandle,
        desc: PortableTextureDesc,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.creates.push(RecordedTextureCreate {
            handle,
            desc,
            pixels: data.to_vec(),
            generation: 0,
        });
        Ok(())
    }

    fn destroy_texture(&mut self, handle: TextureHandle) {
        self.destroys.push(RecordedTextureDestroy { handle });
    }
}

impl RenderBackend for RecordingBackend {
    type Error = RfvpError;

    fn begin_frame(&mut self, _width: u16, _height: u16) -> Result<(), Self::Error> {
        self.begin_frame_calls += 1;
        self.frame = RenderFrame::default();
        Ok(())
    }

    fn submit_commands(&mut self, commands: &[RenderCommand]) -> Result<(), Self::Error> {
        self.frame.commands.extend_from_slice(commands);
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), Self::Error> {
        self.end_frame_calls += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_api::{CommandBlendMode, DrawImageCmd, RectI16, RectU16, Rgba8, Vertex2D};

    #[test]
    fn recording_backend_keeps_texture_and_command_order() {
        let mut backend = RecordingBackend::new();
        backend
            .create_texture(
                TextureHandle(3),
                PortableTextureDesc {
                    width: 1,
                    height: 1,
                    format: crate::host_api::TextureFormat::Rgba8,
                },
                &[255, 0, 0, 255],
            )
            .unwrap();
        backend.begin_frame(16, 9).unwrap();
        backend
            .submit_commands(&[RenderCommand::DrawImage(DrawImageCmd {
                texture: TextureHandle(3),
                src: RectU16::default(),
                dst: RectI16::default(),
                color: Rgba8 {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                blend: CommandBlendMode::Normal,
                effect_id: 0,
                clip: None,
                vertices: [Vertex2D {
                    position: [0.0, 0.0],
                    tex_coord: [0.0, 0.0],
                    color: crate::host_api::ColorRgba {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                }; 4],
            })])
            .unwrap();
        backend.end_frame().unwrap();
        backend.destroy_texture(TextureHandle(3));

        assert_eq!(backend.creates.len(), 1);
        assert_eq!(backend.frame.commands.len(), 1);
        assert_eq!(backend.destroys.len(), 1);
        assert_eq!(backend.begin_frame_calls, 1);
        assert_eq!(backend.end_frame_calls, 1);
    }
}
