//! GPU cursor: the caret is drawn by a WGSL fragment shader over the editor viewport.
//! CPU side only animates four trail corners (Neovide-style) and feeds uniforms.

use crate::config::{config_dir, Config, CursorShape};
use eframe::egui::{self, Pos2, Rect, Vec2};
use eframe::egui_wgpu::{self, wgpu, CallbackTrait};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PRELUDE: &str = include_str!("shaders/prelude.wgsl");

pub const PRESETS: &[(&str, &str)] = &[
    ("trail", include_str!("shaders/trail.wgsl")),
    ("glow", include_str!("shaders/glow.wgsl")),
    ("pulse", include_str!("shaders/pulse.wgsl")),
    ("plain", include_str!("shaders/plain.wgsl")),
];

pub fn custom_shader_path() -> PathBuf {
    config_dir().join("cursor.wgsl")
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FxUniform {
    rect: [f32; 4],
    c01: [f32; 4],
    c23: [f32; 4],
    color: [f32; 4],
    view: [f32; 4],
    state: [f32; 4],
}

struct Gpu {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    bgl: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: Option<wgpu::RenderPipeline>,
    generation: u64,
}

impl Gpu {
    fn build(&mut self, source: &str) -> Result<(), String> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("carbon_cursor"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("carbon_cursor_layout"),
            bind_group_layouts: &[Some(&self.bgl)],
            immediate_size: 0,
        });
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("carbon_cursor_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(wgpu::BlendState { color: blend, alpha: blend }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(e.to_string()),
            None => {
                self.pipeline = Some(pipeline);
                Ok(())
            }
        }
    }
}

struct FxCallback {
    uniform: FxUniform,
    source: Arc<String>,
    generation: u64,
    error: Arc<Mutex<Option<String>>>,
}

impl CallbackTrait for FxCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = res.get_mut::<Gpu>() {
            if gpu.generation != self.generation {
                gpu.generation = self.generation;
                match gpu.build(&self.source) {
                    Ok(()) => *self.error.lock().unwrap() = None,
                    Err(e) => *self.error.lock().unwrap() = Some(e),
                }
            }
            queue.write_buffer(&gpu.buffer, 0, bytemuck::bytes_of(&self.uniform));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = res.get::<Gpu>() {
            if let Some(p) = &gpu.pipeline {
                pass.set_pipeline(p);
                pass.set_bind_group(0, &gpu.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
    }
}

pub struct CursorFx {
    corners: [Pos2; 4],
    have_pos: bool,
    target: Rect,
    last_move: Instant,
    start: Instant,
    pub source: Arc<String>,
    generation: u64,
    effect_key: String,
    error: Arc<Mutex<Option<String>>>,
    last_travel: f32,
}

const REL: [Vec2; 4] = [
    Vec2::new(-1.0, -1.0),
    Vec2::new(1.0, -1.0),
    Vec2::new(1.0, 1.0),
    Vec2::new(-1.0, 1.0),
];

impl CursorFx {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: &Config) -> Self {
        if let Some(rs) = &cc.wgpu_render_state {
            let device = rs.device.clone();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("carbon_cursor_uniform"),
                size: std::mem::size_of::<FxUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("carbon_cursor_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("carbon_cursor_bg"),
                layout: &bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            rs.renderer.write().callback_resources.insert(Gpu {
                device,
                format: rs.target_format,
                bgl,
                buffer,
                bind_group,
                pipeline: None,
                generation: 0,
            });
        }
        let mut s = Self {
            corners: [Pos2::ZERO; 4],
            have_pos: false,
            target: Rect::NOTHING,
            last_move: Instant::now(),
            start: Instant::now(),
            source: Arc::new(String::new()),
            generation: 0,
            effect_key: String::new(),
            error: Arc::new(Mutex::new(None)),
            last_travel: 0.0,
        };
        s.reload_source(cfg);
        s
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }

    fn reload_source(&mut self, cfg: &Config) {
        let body = if cfg.cursor_effect == "custom" {
            let path = custom_shader_path();
            if !path.exists() {
                let _ = std::fs::create_dir_all(config_dir());
                let _ = std::fs::write(&path, PRESETS[0].1);
            }
            std::fs::read_to_string(&path).unwrap_or_else(|_| PRESETS[0].1.to_string())
        } else {
            PRESETS
                .iter()
                .find(|(n, _)| *n == cfg.cursor_effect)
                .unwrap_or(&PRESETS[0])
                .1
                .to_string()
        };
        self.source = Arc::new(format!("{PRELUDE}\n{body}"));
        self.generation += 1;
        self.effect_key = cfg.cursor_effect.clone();
    }

    /// Re-read the shader (preset switch or the custom file changed on disk).
    pub fn reload(&mut self, cfg: &Config) {
        self.reload_source(cfg);
    }

    pub fn effect_changed(&self, cfg: &Config) -> bool {
        self.effect_key != cfg.cursor_effect
    }

    /// Jump straight to the target (tab switch, new file) without a trail.
    pub fn snap(&mut self) {
        self.have_pos = false;
    }

    /// Step the trail animation toward `target` (screen points). Returns true while still moving.
    pub fn step(&mut self, target: Rect, dt: f32) -> bool {
        if target != self.target {
            self.target = target;
            self.last_move = Instant::now();
        }
        let tc = target.center();
        let goal = |i: usize| tc + REL[i] * 0.5 * target.size();
        if !self.have_pos {
            for i in 0..4 {
                self.corners[i] = goal(i);
            }
            self.have_pos = true;
            self.last_travel = 0.0;
            return false;
        }
        let avg = self.corners.iter().fold(Vec2::ZERO, |a, c| a + c.to_vec2()) / 4.0;
        let travel = tc.to_vec2() - avg;
        let dist = travel.length();
        self.last_travel = dist;
        if dist > 400.0 {
            for i in 0..4 {
                self.corners[i] = goal(i);
            }
            return false;
        }
        let dir = if dist > 0.01 { travel / dist } else { Vec2::ZERO };
        let fast = 1.0 - (-dt / 0.03).exp();
        let slow = 1.0 - (-dt / 0.09).exp();
        let mut moving = false;
        for i in 0..4 {
            let align = dir.dot(REL[i].normalized());
            let k = (align + 1.0) * 0.5;
            let rate = slow + (fast - slow) * k;
            let g = goal(i);
            let d = g - self.corners[i];
            if d.length() < 0.15 {
                self.corners[i] = g;
            } else {
                self.corners[i] += d * rate;
                moving = true;
            }
        }
        moving
    }

    pub fn paint(&self, painter: &egui::Painter, view: Rect, cfg: &Config, theme_cursor: [u8; 3], blink_on: bool) {
        let rel = |p: Pos2| [p.x - view.min.x, p.y - view.min.y];
        let (a, b, c, d) = (rel(self.corners[0]), rel(self.corners[1]), rel(self.corners[2]), rel(self.corners[3]));
        let t = self.target;
        let col = cfg.cursor_color.unwrap_or(theme_cursor);
        let shape = match cfg.cursor_shape {
            CursorShape::Bar => 0.0,
            CursorShape::Block => 1.0,
            CursorShape::Underline => 2.0,
        };
        let uniform = FxUniform {
            rect: [t.min.x - view.min.x, t.min.y - view.min.y, t.width(), t.height()],
            c01: [a[0], a[1], b[0], b[1]],
            c23: [c[0], c[1], d[0], d[1]],
            color: [col[0] as f32 / 255.0, col[1] as f32 / 255.0, col[2] as f32 / 255.0, 1.0],
            view: [
                view.width(),
                view.height(),
                self.start.elapsed().as_secs_f32(),
                self.last_move.elapsed().as_secs_f32(),
            ],
            state: [if blink_on { 1.0 } else { 0.0 }, shape, painter.ctx().pixels_per_point(), self.last_travel],
        };
        painter.add(egui_wgpu::Callback::new_paint_callback(
            view,
            FxCallback {
                uniform,
                source: self.source.clone(),
                generation: self.generation,
                error: self.error.clone(),
            },
        ));
    }

    pub fn since_move(&self) -> f32 {
        self.last_move.elapsed().as_secs_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(body: &str) -> Result<(), String> {
        let src = format!("{PRELUDE}\n{body}");
        let module = naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src))?;
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn every_preset_is_valid_wgsl() {
        for (name, body) in PRESETS {
            validate(body).unwrap_or_else(|e| panic!("preset {name} invalid: {e}"));
        }
    }

    #[test]
    fn broken_shader_is_rejected() {
        assert!(validate("fn cursor_fx(p: vec2<f32>) -> vec4<f32> { return vec4(undefined_thing); }").is_err());
        // Missing entry function entirely.
        assert!(validate("fn other() {}").is_err());
    }
}
