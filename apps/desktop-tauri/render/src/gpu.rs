// SPDX-License-Identifier: GPL-3.0-or-later
//! The wgpu renderer. Runs ONLY on its render thread.
//!
//! Two kinds of pass:
//! * the color pass draws the frame (MSAA when the adapter supports 4×):
//!   opaque objects first, then transparent ones back to front with depth
//!   writes off, lit by the VRML97 lighting equation (ISO 4.14.4) with the
//!   default headlight (ISO 6.29: a white directional light along the view
//!   direction, intensity 1, ambientIntensity 0);
//! * the id pass is SEPARATE and never multisampled: on demand, a 1×1 target
//!   with the projection of one pixel of a recorded frame (`View::pick_proj`),
//!   every object written with its pick id, depth tested and depth written.
//!   It draws exactly the triangles of `wrlforge_scene::mesh` the CPU oracle
//!   intersects, with the same culling rule (front face flipped for a
//!   mirrored world matrix).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use glam::{DMat4, DVec3};
use wrlforge_scene::camera::View;
use wrlforge_scene::{mesh::Mesh, RenderScene, Shading};

/// Readback / idle wait limit.
pub const GPU_WAIT: Duration = Duration::from_secs(2);
/// Objects one native scene may hold (uniform memory: `STRIDE` each).
pub const MAX_OBJECTS: usize = 20_000;

pub const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const STRIDE: u64 = 512;
/// Words in one object uniform (see `Obj` in `SHADER`).
const WORDS: usize = 16 * 3 + 4 * 5 + 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentPolicy {
    /// X11 and macOS: FIFO is acceptable (bounded by vsync, render thread only).
    AllowFifo,
    /// Wayland (owner decision D8): Mailbox or Immediate, else refuse.
    RequireNonBlocking,
}

/// The error text `Renderer::new` returns under `RequireNonBlocking` when the
/// surface offers only FIFO. The host turns it into "native disabled".
pub const FIFO_ONLY: &str = "fifo-only";

const SHADER: &str = r#"
struct Obj {
  mvp: mat4x4<f32>,
  model: mat4x4<f32>,
  normal: mat4x4<f32>,
  diffuse: vec4<f32>,   // rgb, alpha
  emissive: vec4<f32>,  // rgb, lit (1) / unlit (0)
  specular: vec4<f32>,  // rgb, shininess
  eye: vec4<f32>,       // world eye, selected (1)
  light: vec4<f32>,     // unit vector towards the headlight, world
  id: vec4<u32>,
};
@group(0) @binding(0) var<uniform> obj: Obj;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) n: vec3<f32>,
  @location(1) w: vec3<f32>,
};

@vertex fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>) -> VOut {
  var o: VOut;
  o.pos = obj.mvp * vec4<f32>(p, 1.0);
  o.n = (obj.normal * vec4<f32>(n, 0.0)).xyz;
  o.w = (obj.model * vec4<f32>(p, 1.0)).xyz;
  return o;
}

// ISO 4.14.4 with one directional light (the headlight, Iia = 0, no fog):
// I = OE + Ii * (OD * (N.L) + OS * (N.H)^(shininess*128)), clamped.
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
  var c = obj.diffuse.rgb;
  if (obj.emissive.w > 0.5) {
    let n = normalize(i.n);
    let l = obj.light.xyz;
    let v = normalize(obj.eye.xyz - i.w);
    let ndl = max(dot(n, l), 0.0);
    let h = normalize(l + v);
    var spec = vec3<f32>(0.0);
    if (ndl > 0.0) {
      spec = obj.specular.rgb * pow(max(dot(n, h), 1e-8), obj.specular.w * 128.0);
    }
    c = obj.emissive.rgb + obj.diffuse.rgb * ndl + spec;
  }
  c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
  if (obj.eye.w > 0.5) {
    c = mix(c, vec3<f32>(1.0, 0.72, 0.18), 0.45);
  }
  return vec4<f32>(c, obj.diffuse.a);
}

@vertex fn vs_id(@location(0) p: vec3<f32>) -> @builtin(position) vec4<f32> {
  return obj.mvp * vec4<f32>(p, 1.0);
}

@fragment fn fs_id() -> @location(0) u32 {
  return obj.id.x;
}
"#;

struct GpuMesh {
    source: Arc<Mesh>,
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    pub backend: String,
    pub adapter: String,
    pub driver: String,
    pub device_type: String,
    pub surface_format: String,
    pub present_mode: String,
    pub msaa: u32,
}

/// What one draw put on screen. A pick names a frame, and is answered with
/// that frame's scene and camera, never the current ones.
#[derive(Debug, Clone)]
pub struct Frame {
    pub seq: u64,
    pub scene: Arc<RenderScene>,
    pub view: View,
    /// Physical pixels per logical pixel when the frame was drawn.
    pub scale: f64,
    /// False when the frame went to the offscreen target (hidden/occluded).
    pub presented: bool,
}

struct Pipes {
    /// [opaque, transparent][counter-clockwise, clockwise front faces].
    color: [[wgpu::RenderPipeline; 2]; 2],
    id: [wgpu::RenderPipeline; 2],
}

struct Uniforms {
    buf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    cap: usize,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    msaa: u32,
    pipes: Pipes,
    bgl: wgpu::BindGroupLayout,
    frame_u: Uniforms,
    pick_u: Uniforms,
    meshes: Vec<GpuMesh>,
    /// Per object of `scene`: index into `meshes`.
    obj_mesh: Vec<usize>,
    scene: Option<Arc<RenderScene>>,
    depth: wgpu::TextureView,
    msaa_color: Option<wgpu::Texture>,
    offscreen: Option<wgpu::Texture>,
    pub info: GpuInfo,
    pub frames: u64,
    pub offscreen_frames: u64,
    /// False while the window is hidden/occluded: no acquire, no present.
    pub present_enabled: bool,
    /// Fault injection: hold the acquired frame this long before present.
    pub hold: Option<Duration>,
    pub lost: Arc<AtomicBool>,
    pub lost_reason: Arc<Mutex<String>>,
    pub uncaptured_errors: Arc<AtomicU64>,
}

fn texture(device: &wgpu::Device, w: u32, h: u32, samples: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn uniforms(device: &wgpu::Device, bgl: &wgpu::BindGroupLayout, cap: usize, label: &str) -> Uniforms {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: STRIDE * cap.max(1) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buf,
                offset: 0,
                size: wgpu::BufferSize::new((WORDS * 4) as u64),
            }),
        }],
    });
    Uniforms { buf, bind, cap: cap.max(1) }
}

fn mat(m: DMat4, out: &mut Vec<u32>) {
    out.extend(m.to_cols_array().iter().map(|x| (*x as f32).to_bits()));
}
fn v4(v: [f32; 4], out: &mut Vec<u32>) {
    out.extend(v.iter().map(|x| x.to_bits()));
}

impl Renderer {
    /// Create the device and configure `surface`. Under
    /// `RequireNonBlocking`, a FIFO-only surface is an `Err` starting with
    /// `FIFO_ONLY` (D8: never a silent FIFO).
    pub fn new(instance: &wgpu::Instance, surface: wgpu::Surface<'static>, width: u32, height: u32, policy: PresentPolicy, force_fifo_only: bool) -> Result<Self, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("request_adapter: {e}"))?;
        let ai = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("wrlforge-native"), ..Default::default() }))
            .map_err(|e| format!("request_device: {e}"))?;
        let lost = Arc::new(AtomicBool::new(false));
        let lost_reason = Arc::new(Mutex::new(String::new()));
        {
            let (l, r) = (lost.clone(), lost_reason.clone());
            device.set_device_lost_callback(move |reason, msg| {
                if let Ok(mut g) = r.lock() {
                    *g = format!("{reason:?}: {msg}");
                }
                l.store(true, Ordering::SeqCst);
            });
        }
        let uncaptured_errors = Arc::new(AtomicU64::new(0));
        {
            let n = uncaptured_errors.clone();
            // The default handler panics; a GPU error must never panic.
            device.on_uncaptured_error(Arc::new(move |_e: wgpu::Error| {
                n.fetch_add(1, Ordering::SeqCst);
            }));
        }
        let mut caps = surface.get_capabilities(&adapter);
        if force_fifo_only {
            caps.present_modes.retain(|m| matches!(m, wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed));
        }
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or(caps.formats.first().copied()).ok_or("surface reports no formats")?;
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied().ok_or("surface reports no alpha modes")?
        };
        let has = |m| caps.present_modes.contains(&m);
        let present_mode = match policy {
            PresentPolicy::RequireNonBlocking if has(wgpu::PresentMode::Mailbox) => wgpu::PresentMode::Mailbox,
            PresentPolicy::RequireNonBlocking if has(wgpu::PresentMode::Immediate) => wgpu::PresentMode::Immediate,
            PresentPolicy::RequireNonBlocking => {
                return Err(format!("{FIFO_ONLY}: no non-blocking present mode (modes {:?})", caps.present_modes));
            }
            PresentPolicy::AllowFifo => wgpu::PresentMode::Fifo,
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let msaa = {
            let f = adapter.get_texture_format_features(format).flags;
            let d = adapter.get_texture_format_features(DEPTH_FORMAT).flags;
            if f.sample_count_supported(4) && d.sample_count_supported(4) && f.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE) { 4 } else { 1 }
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("native"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("obj"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new((WORDS * 4) as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let vbuf = [Some(wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })];
        let make = |id: bool, transparent: bool, cw: bool| {
            let target = if id {
                wgpu::ColorTargetState { format: ID_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }
            } else {
                wgpu::ColorTargetState {
                    format,
                    blend: transparent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                }
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if id { "id" } else { "color" }),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if id { "vs_id" } else { "vs" }),
                    compilation_options: Default::default(),
                    buffers: &vbuf,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    front_face: if cw { wgpu::FrontFace::Cw } else { wgpu::FrontFace::Ccw },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!transparent),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: if id { 1 } else { msaa }, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if id { "fs_id" } else { "fs" }),
                    compilation_options: Default::default(),
                    targets: &[Some(target)],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipes = Pipes {
            color: [[make(false, false, false), make(false, false, true)], [make(false, true, false), make(false, true, true)]],
            id: [make(true, false, false), make(true, false, true)],
        };
        let frame_u = uniforms(&device, &bgl, 1, "frame-objs");
        let pick_u = uniforms(&device, &bgl, 1, "pick-objs");
        let depth = texture(&device, config.width, config.height, msaa, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT, "depth").create_view(&Default::default());
        let msaa_color = (msaa > 1).then(|| texture(&device, config.width, config.height, msaa, format, wgpu::TextureUsages::RENDER_ATTACHMENT, "msaa"));
        let info = GpuInfo {
            backend: format!("{:?}", ai.backend),
            adapter: ai.name.clone(),
            driver: format!("{} {}", ai.driver, ai.driver_info),
            device_type: format!("{:?}", ai.device_type),
            surface_format: format!("{format:?}"),
            present_mode: format!("{:?}", config.present_mode),
            msaa,
        };
        Ok(Renderer {
            device,
            queue,
            surface,
            config,
            msaa,
            pipes,
            bgl,
            frame_u,
            pick_u,
            meshes: Vec::new(),
            obj_mesh: Vec::new(),
            scene: None,
            depth,
            msaa_color,
            offscreen: None,
            info,
            frames: 0,
            offscreen_frames: 0,
            present_enabled: true,
            hold: None,
            lost,
            lost_reason,
            uncaptured_errors,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) == self.size() {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.depth = texture(&self.device, w, h, self.msaa, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT, "depth").create_view(&Default::default());
        if let Some(t) = self.msaa_color.take() {
            t.destroy();
            self.msaa_color = Some(texture(&self.device, w, h, self.msaa, self.config.format, wgpu::TextureUsages::RENDER_ATTACHMENT, "msaa"));
        }
        if let Some(o) = self.offscreen.take() {
            o.destroy();
        }
    }

    /// Upload a new scene. Meshes are shared by identity (`Arc`), so one
    /// tessellation is uploaded once.
    pub fn set_scene(&mut self, scene: Arc<RenderScene>) -> Result<(), String> {
        use wgpu::util::DeviceExt;
        if scene.objects.len() > MAX_OBJECTS {
            return Err(format!("scene has {} objects; the native viewport draws at most {MAX_OBJECTS}", scene.objects.len()));
        }
        let mut meshes: Vec<GpuMesh> = Vec::new();
        let mut obj_mesh = Vec::with_capacity(scene.objects.len());
        for o in &scene.objects {
            if let Some(i) = meshes.iter().position(|m| Arc::ptr_eq(&m.source, &o.mesh)) {
                obj_mesh.push(i);
                continue;
            }
            let m = &o.mesh;
            let mut verts: Vec<f32> = Vec::with_capacity(m.positions.len() * 6);
            for (p, n) in m.positions.iter().zip(&m.normals) {
                verts.extend_from_slice(p);
                verts.extend_from_slice(n);
            }
            let vb = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("vb"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
            let ib = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ib"), contents: bytemuck::cast_slice(&m.indices), usage: wgpu::BufferUsages::INDEX });
            obj_mesh.push(meshes.len());
            meshes.push(GpuMesh { source: m.clone(), vb, ib, count: m.indices.len() as u32 });
        }
        for m in self.meshes.drain(..) {
            m.vb.destroy();
            m.ib.destroy();
        }
        let need = scene.objects.len().max(1);
        if need > self.frame_u.cap {
            self.frame_u.buf.destroy();
            self.pick_u.buf.destroy();
            let cap = need.next_power_of_two().min(MAX_OBJECTS);
            self.frame_u = uniforms(&self.device, &self.bgl, cap, "frame-objs");
            self.pick_u = uniforms(&self.device, &self.bgl, cap, "pick-objs");
        }
        self.meshes = meshes;
        self.obj_mesh = obj_mesh;
        self.scene = Some(scene);
        Ok(())
    }

    pub fn scene(&self) -> Option<&Arc<RenderScene>> {
        self.scene.as_ref()
    }

    /// Per-object uniform words for `vp` (view-projection) and the frame
    /// camera (`eye`, headlight direction `light`).
    fn object_words(o: &wrlforge_scene::RenderObject, vp: DMat4, eye: DVec3, light: DVec3, selected: bool) -> Vec<u32> {
        let mut w = Vec::with_capacity(WORDS);
        mat(vp * o.world, &mut w);
        mat(o.world, &mut w);
        mat(o.world.inverse().transpose(), &mut w);
        let (diffuse, emissive, specular) = match o.shading {
            Shading::Unlit(c) => ([c[0], c[1], c[2], 1.0], [0.0; 4], [0.0; 4]),
            Shading::Lit(m) => (
                [m.diffuse[0], m.diffuse[1], m.diffuse[2], 1.0 - m.transparency],
                [m.emissive[0], m.emissive[1], m.emissive[2], 1.0],
                [m.specular[0], m.specular[1], m.specular[2], m.shininess],
            ),
        };
        v4(diffuse, &mut w);
        v4(emissive, &mut w);
        v4(specular, &mut w);
        v4([eye.x as f32, eye.y as f32, eye.z as f32, if selected { 1.0 } else { 0.0 }], &mut w);
        v4([light.x as f32, light.y as f32, light.z as f32, 0.0], &mut w);
        w.extend_from_slice(&[o.pick_id, 0, 0, 0]);
        w
    }

    fn write_objects(&self, u: &Uniforms, vp: DMat4, view: &View, selected: &[u32]) -> Result<(), String> {
        let scene = self.scene.as_ref().ok_or("no scene")?;
        let inv = view.view.inverse();
        let eye = inv.transform_point3(DVec3::ZERO);
        // The headlight shines along the view direction: towards it is +Z of
        // the camera frame.
        let light = inv.transform_vector3(DVec3::Z).normalize_or_zero();
        let mut bytes: Vec<u8> = vec![0; STRIDE as usize * scene.objects.len().max(1)];
        for (k, o) in scene.objects.iter().enumerate() {
            let w = Self::object_words(o, vp, eye, light, selected.contains(&o.pick_id));
            let at = k * STRIDE as usize;
            bytes[at..at + WORDS * 4].copy_from_slice(bytemuck::cast_slice(&w));
        }
        self.queue.write_buffer(&u.buf, 0, &bytes);
        Ok(())
    }

    /// Draw one frame of the current scene and return its record.
    /// `selected`: pick ids drawn with the selection tint.
    pub fn draw(&mut self, view: &View, scale: f64, selected: &[u32]) -> Result<Frame, String> {
        if self.is_lost() {
            return Err("device lost".into());
        }
        let scene = self.scene.clone().ok_or("no scene")?;
        let (w, h) = self.size();
        self.write_objects(&self.frame_u, view.view_proj(), view, selected)?;
        // Hidden window: never acquire (a hidden Wayland/macOS surface can
        // block acquire/present); draw offscreen so the frame record exists.
        let frame = if !self.present_enabled {
            None
        } else {
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    self.surface.configure(&self.device, &self.config);
                    None
                }
                _ => None,
            }
        };
        if frame.is_none() && self.offscreen.is_none() {
            self.offscreen = Some(texture(&self.device, w, h, 1, self.config.format, wgpu::TextureUsages::RENDER_ATTACHMENT, "offscreen"));
        }
        let target = match (&frame, &self.offscreen) {
            (Some(f), _) => f.texture.create_view(&Default::default()),
            (None, Some(o)) => o.create_view(&Default::default()),
            (None, None) => return Err("no render target".into()),
        };
        let msaa_view = self.msaa_color.as_ref().map(|t| t.create_view(&Default::default()));
        let (view_tex, resolve) = match &msaa_view {
            Some(m) => (m, Some(&target)),
            None => (&target, None),
        };
        // Opaque in scene order, then transparent back to front.
        let vm = view.view;
        let depth_of = |k: usize| vm.transform_point3(scene.objects[k].world.transform_point3(DVec3::ZERO)).z;
        let (mut opaque, mut transparent): (Vec<usize>, Vec<usize>) = (0..scene.objects.len()).partition(|&k| scene.objects[k].shading.alpha() >= 1.0);
        opaque.sort_unstable();
        transparent.sort_by(|&a, &b| depth_of(a).total_cmp(&depth_of(b)));
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("color"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_tex,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
                        store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for (group, list) in [(0usize, &opaque), (1, &transparent)] {
                for &k in list.iter() {
                    let o = &scene.objects[k];
                    let Some(m) = self.obj_mesh.get(k).and_then(|&i| self.meshes.get(i)) else { continue };
                    pass.set_pipeline(&self.pipes.color[group][usize::from(o.mirrored)]);
                    pass.set_bind_group(0, &self.frame_u.bind, &[(k as u64 * STRIDE) as u32]);
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                }
            }
        }
        self.queue.submit([enc.finish()]);
        let presented = frame.is_some();
        if let (Some(d), Some(_)) = (self.hold.take(), &frame) {
            std::thread::sleep(d); // fault injection: a present that does not return
        }
        match frame {
            Some(f) => self.queue.present(f),
            None => self.offscreen_frames += 1,
        }
        self.frames += 1;
        Ok(Frame { seq: self.frames, scene, view: *view, scale, presented })
    }

    /// Render the id pass with `proj · frame.view` into a `w`×`h` target and
    /// read it back. The frame's scene must still be the uploaded one.
    fn id_pass(&self, frame: &Frame, proj: DMat4, w: u32, h: u32) -> Result<Vec<u32>, String> {
        if self.is_lost() {
            return Err("device lost".into());
        }
        let scene = self.scene.as_ref().ok_or("no scene")?;
        if !Arc::ptr_eq(scene, &frame.scene) {
            return Err("frame scene replaced".into());
        }
        self.write_objects(&self.pick_u, proj * frame.view.view, &frame.view, &[])?;
        let ids = texture(&self.device, w, h, 1, ID_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, "ids");
        let depth = texture(&self.device, w, h, 1, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT, "id-depth");
        let (idv, dv) = (ids.create_view(&Default::default()), depth.create_view(&Default::default()));
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("id-readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("id"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &idv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &dv,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for (k, o) in scene.objects.iter().enumerate() {
                let Some(m) = self.obj_mesh.get(k).and_then(|&i| self.meshes.get(i)) else { continue };
                pass.set_pipeline(&self.pipes.id[usize::from(o.mirrored)]);
                pass.set_bind_group(0, &self.pick_u.bind, &[(k as u64 * STRIDE) as u32]);
                pass.set_vertex_buffer(0, m.vb.slice(..));
                pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
            }
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &ids, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let data = self.read_buf(&buf, (row * h) as usize);
        ids.destroy();
        depth.destroy();
        buf.destroy();
        let data = data?;
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let o = y * row as usize + x * 4;
                out.push(u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]));
            }
        }
        Ok(out)
    }

    /// The GPU id at physical pixel (`px`, `py`) of `frame`: a 1×1 id pass
    /// with that pixel's projection and the frame's own camera.
    pub fn pick_id(&self, frame: &Frame, px: u32, py: u32) -> Result<u32, String> {
        if px >= frame.view.width || py >= frame.view.height {
            return Err("pick outside the frame".into());
        }
        let ids = self.id_pass(frame, frame.view.pick_proj(px, py), 1, 1)?;
        ids.first().copied().ok_or_else(|| "empty id readback".into())
    }

    /// The full-size id image of `frame` (test evidence only).
    pub fn id_image(&self, frame: &Frame) -> Result<Vec<u32>, String> {
        self.id_pass(frame, frame.view.proj, frame.view.width, frame.view.height)
    }

    /// Bounded readback: a map error, a poll timeout or a lost device is an
    /// `Err`, never a panic and never an endless wait.
    fn read_buf(&self, buf: &wgpu::Buffer, n: usize) -> Result<Vec<u8>, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(GPU_WAIT) })
            .map_err(|e| format!("poll: {e}"))?;
        match rx.try_recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("map_async: {e}")),
            Err(_) => return Err("map_async: no result after poll".into()),
        }
        let out = {
            let view = buf.slice(..).get_mapped_range().map_err(|e| format!("mapped range: {e:?}"))?;
            if view.len() < n {
                return Err("short readback".into());
            }
            view[..n].to_vec()
        };
        buf.unmap();
        Ok(out)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // GPU work done (with a limit), then resources, then the device. The
        // surface field drops after this body, before the host's connection.
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(GPU_WAIT) });
        if let Some(o) = self.offscreen.take() {
            o.destroy();
        }
        if let Some(t) = self.msaa_color.take() {
            t.destroy();
        }
        self.frame_u.buf.destroy();
        self.pick_u.buf.destroy();
        for m in &self.meshes {
            m.vb.destroy();
            m.ib.destroy();
        }
        self.device.destroy();
    }
}
