use crate::{
    NodeId,
    ui::{Bounds, Color, Frame, Paint},
};
use glyphon::{
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    sync::Arc,
};
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
    local: [f32; 2],
    half_size: [f32; 2],
    color: Color,
    border_color: Color,
    parameters: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
enum Draw {
    Primitives {
        start: u32,
        end: u32,
        clip: (u32, u32, u32, u32),
    },
    Text(NodeId),
}

fn pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("finite primitives"),
        source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("finite primitives"), layout: None,
        vertex: wgpu::VertexState {
            module: &shader, entry_point: Some("vertex"), compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader, entry_point: Some("fragment"), compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview: None, cache: None,
    })
}

fn compile(
    paints: &[Paint],
    scale: f32,
    width: u32,
    height: u32,
    batch: bool,
) -> (Vec<Vertex>, Vec<Draw>) {
    let mut vertices = Vec::new();
    let mut draws = Vec::new();
    for paint in paints {
        let (bounds, clip, color, radius, border, blur, rotation) = match *paint {
            Paint::Text(id) => {
                draws.push(Draw::Text(id));
                continue;
            }
            Paint::Rect {
                bounds,
                clip,
                color,
            } => (bounds, clip, color, 0.0, None, 0.0, [1.0, 0.0]),
            Paint::Shape {
                bounds,
                clip,
                color,
                radius,
                border,
                blur,
            } => (bounds, clip, color, radius, border, blur, [1.0, 0.0]),
            Paint::Segment {
                origin,
                clip,
                segment,
            } => {
                let delta = [
                    segment.to[0] - segment.from[0],
                    segment.to[1] - segment.from[1],
                ];
                let length = delta[0].hypot(delta[1]);
                let rotation = if length > 0.0 {
                    [delta[0] / length, delta[1] / length]
                } else {
                    [1.0, 0.0]
                };
                let center = [
                    origin[0] + (segment.from[0] + segment.to[0]) * 0.5,
                    origin[1] + (segment.from[1] + segment.to[1]) * 0.5,
                ];
                let bounds = Bounds {
                    x: center[0] - (length + segment.width) * 0.5,
                    y: center[1] - segment.width * 0.5,
                    width: length + segment.width,
                    height: segment.width,
                };
                (
                    bounds,
                    clip,
                    segment.color,
                    segment.width * 0.5,
                    None,
                    0.0,
                    rotation,
                )
            }
        };
        let clip = physical_clip(clip, scale, width, height);
        if bounds.width <= 0.0 || bounds.height <= 0.0 || clip.2 == 0 || clip.3 == 0 {
            continue;
        }
        let half_size = [bounds.width * 0.5, bounds.height * 0.5];
        let limit = half_size[0].min(half_size[1]);
        let border_width = border.map_or(0.0, |border| border.width.min(limit));
        let margin = blur + 1.0 / scale;
        let start = vertices.len() as u32;
        for corner in [
            [-1.0, -1.0],
            [-1.0, 1.0],
            [1.0, -1.0],
            [1.0, -1.0],
            [-1.0, 1.0],
            [1.0, 1.0],
        ] {
            let local = [
                corner[0] * (half_size[0] + margin),
                corner[1] * (half_size[1] + margin),
            ];
            let position = [
                bounds.x + half_size[0] + rotation[0] * local[0] - rotation[1] * local[1],
                bounds.y + half_size[1] + rotation[1] * local[0] + rotation[0] * local[1],
            ];
            vertices.push(Vertex {
                position: [
                    position[0] * scale / width as f32 * 2.0 - 1.0,
                    1.0 - position[1] * scale / height as f32 * 2.0,
                ],
                local,
                half_size,
                color: crate::visual::linear(color),
                border_color: crate::visual::linear(border.map_or([0.0; 4], |border| border.color)),
                parameters: [radius.min(limit), border_width, blur, 0.0],
            });
        }
        let end = vertices.len() as u32;
        if batch
            && let Some(Draw::Primitives {
                end: last,
                clip: previous,
                ..
            }) = draws.last_mut()
            && *previous == clip
            && *last == start
        {
            *last = end;
            continue;
        }
        draws.push(Draw::Primitives { start, end, clip });
    }
    (vertices, draws)
}

struct PreparedText {
    renderer: TextRenderer,
    version: u64,
}

struct VertexStorage {
    buffer: wgpu::Buffer,
    capacity: u64,
    allocations: usize,
}

impl VertexStorage {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: Self::allocate(device, 256),
            capacity: 256,
            allocations: 1,
        }
    }

    fn allocate(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("reusable primitives"),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vertices: &[Vertex]) -> usize {
        let bytes = bytemuck::cast_slice(vertices);
        if bytes.is_empty() {
            return 0;
        }
        if bytes.len() as u64 > self.capacity {
            self.capacity = (bytes.len() as u64).next_power_of_two();
            self.buffer = Self::allocate(device, self.capacity);
            self.allocations += 1;
        }
        queue.write_buffer(&self.buffer, 0, bytes);
        bytes.len()
    }
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
    texts: BTreeMap<NodeId, PreparedText>,
    vertices: VertexStorage,
    text_high_water: usize,
    prepared_total: usize,
    text_created: usize,
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
            .ok_or("Chronobreak requires an sRGB surface format")?;
        surface.configure(&device, &config);
        if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
            eprintln!("surface format={:?} color_mode=Accurate", config.format);
        }
        let pipeline = pipeline(&device, config.format);
        let cache = Cache::new(&device);
        let atlas = TextAtlas::with_color_mode(
            &device,
            &queue,
            &cache,
            config.format,
            glyphon::ColorMode::Accurate,
        );
        let viewport = Viewport::new(&device, &cache);
        let vertices = VertexStorage::new(&device);
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
            vertices,
            text_high_water: 0,
            prepared_total: 0,
            text_created: 0,
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

    pub fn collect_unmounted(&mut self, live: &BTreeSet<NodeId>) {
        retain_text_resources(&mut self.texts, live);
    }

    pub fn draw(&mut self, frame_view: Frame<'_>, scale: f32) -> Result<DrawOutcome> {
        let drawable: BTreeSet<_> = frame_view.texts.keys().copied().collect();
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
        let prepare_started = std::time::Instant::now();
        for (index, resource) in &frame_view.texts {
            if !self.texts.contains_key(index) {
                self.text_created += 1;
            }
            let buffer = resource.buffer;
            let origin = resource.origin;
            let inner_clip = resource.clip;
            let clip = physical_clip(inner_clip, scale, self.config.width, self.config.height);
            let prepared = self.texts.entry(*index).or_insert_with(|| PreparedText {
                renderer: TextRenderer::new(
                    &mut self.atlas,
                    &self.device,
                    Default::default(),
                    None,
                ),
                version: 0,
            });
            prepared.renderer.prepare(
                &self.device,
                &self.queue,
                frame_view.fonts,
                &mut self.atlas,
                &self.viewport,
                [TextArea {
                    buffer,
                    left: origin.0 * scale,
                    top: origin.1 * scale,
                    scale,
                    bounds: TextBounds {
                        left: clip.0 as i32,
                        top: clip.1 as i32,
                        right: (clip.0 + clip.2) as i32,
                        bottom: (clip.1 + clip.3) as i32,
                    },
                    default_color: glyphon::Color::rgba(
                        (resource.color[0] * 255.0).round() as u8,
                        (resource.color[1] * 255.0).round() as u8,
                        (resource.color[2] * 255.0).round() as u8,
                        (resource.color[3] * 255.0).round() as u8,
                    ),
                    custom_glyphs: &[],
                }],
                &mut self.swash,
            )?;
            self.prepared_total += 1;
            prepared.version = resource.version;
        }
        self.text_high_water = self.text_high_water.max(self.texts.len());
        if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
            eprintln!(
                "resources prepared={} skipped={} text_cache={} text_high_water={} prepared_total={} text_created={} prepare_us={}",
                drawable.len(),
                frame_view.text_buffers - drawable.len(),
                self.texts.len(),
                self.text_high_water,
                self.prepared_total,
                self.text_created,
                prepare_started.elapsed().as_micros()
            );
        }
        let (vertices, draws) = compile(
            &frame_view.commands,
            scale,
            self.config.width,
            self.config.height,
            std::env::var_os("CHRONOBREAK_UNBATCHED").is_none(),
        );
        let uploaded = self.vertices.upload(&self.device, &self.queue, &vertices);
        let mut primitive_draws = 0;
        let mut text_render_calls = 0;
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
            for draw in draws {
                match draw {
                    Draw::Primitives {
                        start,
                        end,
                        clip: (x, y, width, height),
                    } => {
                        pass.set_scissor_rect(x, y, width, height);
                        pass.set_pipeline(&self.pipeline);
                        pass.set_vertex_buffer(0, self.vertices.buffer.slice(..));
                        pass.draw(start..end, 0..1);
                        primitive_draws += 1;
                    }
                    Draw::Text(index) => {
                        pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                        let Some(text) = self.texts.get(&index) else {
                            eprintln!("Chronobreak skipped stale text resource: {index:?}");
                            continue;
                        };
                        if frame_view
                            .texts
                            .get(&index)
                            .is_none_or(|resource| resource.version != text.version)
                        {
                            return Err("stale text resource version".into());
                        }
                        text.renderer
                            .render(&self.atlas, &self.viewport, &mut pass)?;
                        text_render_calls += 1;
                    }
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
            eprintln!(
                "render primitive_draws={primitive_draws} text_render_calls={text_render_calls} primitives={} upload_bytes={uploaded} upload_calls={} vertex_capacity={} vertex_allocations={}",
                vertices.len() / 6,
                usize::from(uploaded > 0),
                self.vertices.capacity,
                self.vertices.allocations
            );
        }
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

    fn rect(clip: Bounds) -> Paint {
        Paint::Rect {
            bounds: Bounds {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            clip,
            color: [0.5, 0.5, 0.5, 0.5],
        }
    }

    #[test]
    fn batching_only_merges_consecutive_compatible_primitives() {
        let mut ids = crate::node::Arena::new();
        let first = ids.insert(());
        let last = ids.insert(());
        let clip = Bounds {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let other = Bounds {
            width: 50.0,
            ..clip
        };
        let paints = [
            rect(clip),
            rect(clip),
            Paint::Text(first),
            rect(clip),
            Paint::Text(last),
            rect(other),
            rect(clip),
        ];
        let (vertices, draws) = compile(&paints, 1.5, 150, 150, true);
        assert_eq!(vertices.len(), 30);
        assert!(
            matches!(draws.as_slice(), [Draw::Primitives { start: 0, end: 12, .. }, Draw::Text(a), Draw::Primitives { start: 12, end: 18, .. }, Draw::Text(b), Draw::Primitives { start: 18, end: 24, .. }, Draw::Primitives { start: 24, end: 30, .. }] if *a == first && *b == last)
        );
        let (unbatched, reference_draws) = compile(&paints, 1.5, 150, 150, false);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&vertices),
            bytemuck::cast_slice::<_, u8>(&unbatched)
        );
        assert_eq!(reference_draws.len(), 7);
    }

    #[test]
    fn contiguous_primitive_plan_has_one_draw_and_clamped_geometry() {
        let bounds = Bounds {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 20.0,
        };
        let paints = vec![
            Paint::Shape {
                bounds,
                clip: Bounds {
                    width: 100.0,
                    height: 100.0,
                    ..Bounds::default()
                },
                color: [0.5; 4],
                radius: 4096.0,
                border: Some(crate::Border::new(4096.0, [1.0; 4])),
                blur: 0.0
            };
            32
        ];
        for scale in [1.0, 1.5, 2.0] {
            let (vertices, draws) = compile(&paints, scale, 200, 200, true);
            assert_eq!(draws.len(), 1);
            assert_eq!(compile(&paints, scale, 200, 200, false).1.len(), 32);
            assert_eq!(vertices[0].parameters, [10.0, 10.0, 0.0, 0.0]);
            assert!(vertices.iter().all(|vertex| {
                vertex
                    .position
                    .iter()
                    .all(|coordinate| coordinate.is_finite())
            }));
        }
    }

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
