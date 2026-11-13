use crate::{
    NodeId,
    ui::{Bounds, Color, Paint, Scene},
};
use glyphon::{
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    sync::Arc,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) fn retain_text_resources<T>(
    resources: &mut BTreeMap<NodeId, T>,
    drawable: &BTreeSet<NodeId>,
) {
    resources.retain(|id, _| drawable.contains(id));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawOutcome {
    Presented,
    Retry,
}

fn acquire_frame<T>(
    acquired: std::result::Result<T, wgpu::SurfaceError>,
    reconfigure: impl FnOnce(),
) -> Result<Option<T>> {
    match acquired {
        Ok(frame) => Ok(Some(frame)),
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            reconfigure();
            Ok(None)
        }
        Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: Color,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    atlas: TextAtlas,
    viewport: Viewport,
    swash: SwashCache,
    texts: BTreeMap<NodeId, TextRenderer>,
    text_high_water: usize,
    prepared_total: usize,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        eprintln!("GPU adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("no supported surface configuration")?;
        let formats = surface.get_capabilities(&adapter).formats;
        config.format = formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(config.format);
        surface.configure(&device, &config);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rectangles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rectangles"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let cache = Cache::new(&device);
        let atlas = TextAtlas::new(&device, &queue, &cache, config.format);
        let viewport = Viewport::new(&device, &cache);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            atlas,
            viewport,
            swash: SwashCache::new(),
            texts: BTreeMap::new(),
            text_high_water: 0,
            prepared_total: 0,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn collect_unmounted(&mut self, scene: &Scene) {
        self.texts.retain(|id, _| {
            scene
                .nodes
                .get(*id)
                .is_some_and(|node| node.buffer.is_some())
        });
    }

    pub fn draw(&mut self, scene: &mut Scene, scale: f32) -> Result<DrawOutcome> {
        let paints = scene.display_list();
        let drawable: BTreeSet<_> = paints
            .iter()
            .filter_map(|paint| match paint {
                Paint::Text(id) if scene.nodes.get(*id).is_some() => Some(*id),
                _ => None,
            })
            .collect();
        retain_text_resources(&mut self.texts, &drawable);
        let Some(frame) = acquire_frame(self.surface.get_current_texture(), || {
            self.surface.configure(&self.device, &self.config);
        })?
        else {
            return Ok(DrawOutcome::Retry);
        };
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        for index in &drawable {
            let node = &scene.nodes[*index];
            if let Some(buffer) = &node.buffer {
                let clip = physical_clip(node.clip, scale, self.config.width, self.config.height);
                self.texts
                    .entry(*index)
                    .or_insert_with(|| {
                        TextRenderer::new(&mut self.atlas, &self.device, Default::default(), None)
                    })
                    .prepare(
                        &self.device,
                        &self.queue,
                        &mut scene.fonts,
                        &mut self.atlas,
                        &self.viewport,
                        [TextArea {
                            buffer,
                            left: node.bounds.x * scale,
                            top: node.bounds.y * scale,
                            scale,
                            bounds: TextBounds {
                                left: clip.0 as i32,
                                top: clip.1 as i32,
                                right: (clip.0 + clip.2) as i32,
                                bottom: (clip.1 + clip.3) as i32,
                            },
                            default_color: glyphon::Color::rgba(
                                (node.color[0] * 255.0) as u8,
                                (node.color[1] * 255.0) as u8,
                                (node.color[2] * 255.0) as u8,
                                (node.color[3] * 255.0) as u8,
                            ),
                            custom_glyphs: &[],
                        }],
                        &mut self.swash,
                    )?;
                self.prepared_total += 1;
            }
        }
        self.text_high_water = self.text_high_water.max(self.texts.len());
        if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
            let buffers = scene
                .nodes
                .iter()
                .filter(|node| node.buffer.is_some())
                .count();
            eprintln!(
                "resources {} prepared={} skipped={} text_cache={} text_high_water={} prepared_total={}",
                scene.stats(),
                drawable.len(),
                buffers - drawable.len(),
                self.texts.len(),
                self.text_high_water,
                self.prepared_total
            );
        }
        let mut vertices = Vec::new();
        let width = self.config.width as f32 / scale;
        let height = self.config.height as f32 / scale;
        for paint in &paints {
            if let Paint::Rect { bounds, color, .. } = *paint {
                let left = bounds.x / width * 2.0 - 1.0;
                let right = (bounds.x + bounds.width) / width * 2.0 - 1.0;
                let top = 1.0 - bounds.y / height * 2.0;
                let bottom = 1.0 - (bounds.y + bounds.height) / height * 2.0;
                for position in [
                    [left, top],
                    [left, bottom],
                    [right, top],
                    [right, top],
                    [left, bottom],
                    [right, bottom],
                ] {
                    vertices.push(Vertex { position, color });
                }
            }
        }
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("display list rectangles"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ordered UI"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.025,
                            g: 0.035,
                            b: 0.06,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut vertex_offset = 0;
            for paint in paints {
                match paint {
                    Paint::Rect { clip, .. } => {
                        let (x, y, width, height) =
                            physical_clip(clip, scale, self.config.width, self.config.height);
                        if width > 0 && height > 0 {
                            pass.set_scissor_rect(x, y, width, height);
                            pass.set_pipeline(&self.pipeline);
                            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                            pass.draw(vertex_offset..vertex_offset + 6, 0..1);
                        }
                        vertex_offset += 6;
                    }
                    Paint::Text(index) => {
                        pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                        let Some(text) = self.texts.get(&index) else {
                            eprintln!("Chronobreak skipped stale text resource: {index:?}");
                            continue;
                        };
                        text.render(&self.atlas, &self.viewport, &mut pass)?;
                    }
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.atlas.trim();
        Ok(DrawOutcome::Presented)
    }
}

fn physical_clip(bounds: Bounds, scale: f32, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let left = (bounds.x * scale).floor().max(0.0).min(width as f32) as u32;
    let top = (bounds.y * scale).floor().max(0.0).min(height as f32) as u32;
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return (left, top, 0, 0);
    }
    let right = ((bounds.x + bounds.width) * scale)
        .ceil()
        .max(left as f32)
        .min(width as f32) as u32;
    let bottom = ((bounds.y + bounds.height) * scale)
        .ceil()
        .max(top as f32)
        .min(height as f32) as u32;
    (left, top, right - left, bottom - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_acquisition_errors_request_retry() {
        for error in [wgpu::SurfaceError::Timeout, wgpu::SurfaceError::Other] {
            assert!(
                acquire_frame::<()>(Err(error), || panic!("unexpected reconfigure"))
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn reconfiguration_and_subsequent_temporary_failure_remain_retryable() {
        for error in [wgpu::SurfaceError::Lost, wgpu::SurfaceError::Outdated] {
            let mut reconfigured = false;
            assert!(
                acquire_frame::<()>(Err(error), || reconfigured = true)
                    .unwrap()
                    .is_none()
            );
            assert!(reconfigured);
            assert!(
                acquire_frame::<()>(Err(wgpu::SurfaceError::Timeout), || {
                    panic!("unexpected reconfigure")
                })
                .unwrap()
                .is_none()
            );
            assert_eq!(acquire_frame(Ok(42), || {}).unwrap(), Some(42));
        }
    }

    #[test]
    fn unrecoverable_acquisition_error_is_preserved() {
        let error = acquire_frame::<()>(Err(wgpu::SurfaceError::OutOfMemory), || {}).unwrap_err();
        assert_eq!(
            error.downcast_ref::<wgpu::SurfaceError>(),
            Some(&wgpu::SurfaceError::OutOfMemory)
        );
    }

    #[test]
    fn fractional_dpi_clipping_is_bounded_in_physical_pixels() {
        assert_eq!(
            physical_clip(
                Bounds {
                    x: 1.25,
                    y: 2.25,
                    width: 0.0,
                    height: 0.0
                },
                1.5,
                100,
                100
            ),
            (1, 3, 0, 0)
        );
        assert_eq!(
            physical_clip(
                Bounds {
                    x: 1.0,
                    y: 2.0,
                    width: 10.0,
                    height: 20.0
                },
                1.5,
                100,
                100
            ),
            (1, 3, 16, 30)
        );
        assert_eq!(
            physical_clip(
                Bounds {
                    x: 90.0,
                    y: 90.0,
                    width: 20.0,
                    height: 20.0
                },
                2.0,
                100,
                100
            ),
            (100, 100, 0, 0)
        );
    }
}
