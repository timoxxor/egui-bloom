#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![expect(rustdoc::missing_crate_level_docs)]
#![expect(unsafe_code)]
#![expect(clippy::undocumented_unsafe_blocks)]

use eframe::{egui, egui_glow, glow};

use egui::mutex::Mutex;
use std::sync::Arc;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 400.0]),
        multisampling: 4,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Свет",
        options,
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

struct MyApp {
    bloom: Arc<Mutex<BloomEffect>>,
}

impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let gl = cc.gl.as_ref().expect("Need glow backend");
        Self {
            bloom: Arc::new(Mutex::new(BloomEffect::new(gl))),
        }
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show_inside(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                let bloom = Arc::clone(&self.bloom);

                let callback = egui::PaintCallback {
                    rect,
                    callback: std::sync::Arc::new(egui_glow::CallbackFn::new(
                        move |_info, painter| {
                            bloom.lock().paint(painter.gl(), rect);
                        },
                    )),
                };
                ui.painter().add(callback);
            });
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        if let Some(gl) = gl {
            self.bloom.lock().destroy(gl);
        }
    }
}

struct BloomEffect {
    text_tex: glow::Texture,
    fbo_a: glow::Framebuffer,
    tex_a: glow::Texture,
    fbo_b: glow::Framebuffer,
    tex_b: glow::Texture,
    prog_copy: glow::Program,
    prog_sharp: glow::Program,
    prog_blur_h: glow::Program,
    prog_blur_v: glow::Program,
    prog_circle: glow::Program,
    vao: glow::VertexArray,
    width: u32,
    height: u32,
}

impl BloomEffect {
    fn new(gl: &glow::Context) -> Self {
        use glow::HasContext as _;

        let width = 400;
        let height = 400;

        let shader_version = if cfg!(target_arch = "wasm32") {
            "#version 300 es"
        } else {
            "#version 330"
        };

        unsafe {
            let text_tex = Self::create_text_texture(gl, width, height);
            let vao = gl.create_vertex_array().expect("Cannot create VAO");
            let (fbo_a, tex_a) = Self::create_fbo(gl, width, height);
            let (fbo_b, tex_b) = Self::create_fbo(gl, width, height);

            let prog_copy = Self::compile_program(
                gl,
                shader_version,
                "
                    out vec2 v_uv;
                    void main() {
                        const vec2 verts[3] = vec2[3](
                            vec2(-1.0, -1.0),
                            vec2(3.0, -1.0),
                            vec2(-1.0, 3.0)
                        );
                        v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                    }
                ",
                "
                    precision mediump float;
                    in vec2 v_uv;
                    uniform sampler2D u_tex;
                    uniform float u_intensity;
                    out vec4 out_color;
                    void main() {
                        float v = texture(u_tex, v_uv).r;
                        out_color = vec4(vec3(v * u_intensity), 1.0);
                    }
                ",
            );

            let prog_sharp = Self::compile_program(
                gl,
                shader_version,
                "
                    out vec2 v_uv;
                    void main() {
                        const vec2 verts[3] = vec2[3](
                            vec2(-1.0, -1.0),
                            vec2(3.0, -1.0),
                            vec2(-1.0, 3.0)
                        );
                        v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                    }
                ",
                "
                    precision mediump float;
                    in vec2 v_uv;
                    uniform sampler2D u_tex;
                    out vec4 out_color;
                    void main() {
                        float v = texture(u_tex, v_uv).r;
                        out_color = vec4(vec3(v), v);
                    }
                ",
            );

            let prog_blur_h = Self::compile_program(
                gl,
                shader_version,
                "
                    out vec2 v_uv;
                    void main() {
                        const vec2 verts[3] = vec2[3](
                            vec2(-1.0, -1.0),
                            vec2(3.0, -1.0),
                            vec2(-1.0, 3.0)
                        );
                        v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                    }
                ",
                "
                    precision mediump float;
                    in vec2 v_uv;
                    uniform sampler2D u_tex;
                    uniform vec2 u_texel;
                    out vec4 out_color;
                    void main() {
                        vec4 c = texture(u_tex, v_uv) * 0.40;
                        c += texture(u_tex, v_uv + vec2( u_texel.x * 2.0, 0.0)) * 0.22;
                        c += texture(u_tex, v_uv - vec2( u_texel.x * 2.0, 0.0)) * 0.22;
                        c += texture(u_tex, v_uv + vec2( u_texel.x * 5.0, 0.0)) * 0.08;
                        c += texture(u_tex, v_uv - vec2( u_texel.x * 5.0, 0.0)) * 0.08;
                        out_color = c;
                    }
                ",
            );

            let prog_blur_v = Self::compile_program(
                gl,
                shader_version,
                "
                    out vec2 v_uv;
                    void main() {
                        const vec2 verts[3] = vec2[3](
                            vec2(-1.0, -1.0),
                            vec2(3.0, -1.0),
                            vec2(-1.0, 3.0)
                        );
                        v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                    }
                ",
                "
                    precision mediump float;
                    in vec2 v_uv;
                    uniform sampler2D u_tex;
                    uniform vec2 u_texel;
                    out vec4 out_color;
                    void main() {
                        vec4 c = texture(u_tex, v_uv) * 0.40;
                        c += texture(u_tex, v_uv + vec2(0.0,  u_texel.y * 2.0)) * 0.22;
                        c += texture(u_tex, v_uv - vec2(0.0,  u_texel.y * 2.0)) * 0.22;
                        c += texture(u_tex, v_uv + vec2(0.0,  u_texel.y * 5.0)) * 0.08;
                        c += texture(u_tex, v_uv - vec2(0.0,  u_texel.y * 5.0)) * 0.08;
                        out_color = c;
                    }
                ",
            );

            let prog_circle = Self::compile_program(
                gl,
                shader_version,
                "
                    out vec2 v_pos;
                    void main() {
                        const vec2 verts[3] = vec2[3](
                            vec2(-1.0, -1.0),
                            vec2(3.0, -1.0),
                            vec2(-1.0, 3.0)
                        );
                        v_pos = verts[gl_VertexID];
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                    }
                ",
                "
                    precision mediump float;
                    in vec2 v_pos;
                    uniform float u_aspect;
                    out vec4 out_color;
                    void main() {
                        vec2 p = vec2(v_pos.x * u_aspect, v_pos.y);
                        float d = length(p);

                        float glow = exp(-d * 2.5) * 0.55;
                        float core = exp(-d * 10.0) * 0.25;

                        float brightness = glow + core;
                        out_color = vec4(vec3(brightness), 1.0);
                    }
                ",
            );

            Self {
                text_tex,
                fbo_a,
                tex_a,
                fbo_b,
                tex_b,
                prog_copy,
                prog_sharp,
                prog_blur_h,
                prog_blur_v,
                prog_circle,
                vao,
                width,
                height,
            }
        }
    }

    fn create_text_texture(gl: &glow::Context, buf_w: u32, buf_h: u32) -> glow::Texture {
        use glow::HasContext as _;
        use ab_glyph::{point, Font as _, ScaleFont as _};

        let text = "Glow";
        let font_path = "C:\\Windows\\Fonts\\arial.ttf";

        let font_data = std::fs::read(font_path).expect("Cannot read font file");
        let font = ab_glyph::FontArc::try_from_vec(font_data).expect("Invalid font");

        let scale_px = 72.0f32;
        let scaled_font = font.as_scaled(ab_glyph::PxScale::from(scale_px));

        let mut overall: Option<ab_glyph::Rect> = None;
        let mut outlines: Vec<ab_glyph::OutlinedGlyph> = Vec::new();
        let mut cursor_x = 0.0f32;
        for c in text.chars() {
            let g = font
                .glyph_id(c)
                .with_scale_and_position(scale_px, point(cursor_x, 0.0));
            cursor_x += scaled_font.h_advance(g.id);
            if let Some(outline) = font.outline_glyph(g) {
                let b = outline.px_bounds();
                overall = Some(match overall {
                    None => b,
                    Some(a) => ab_glyph::Rect {
                        min: point(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
                        max: point(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
                    },
                });
                outlines.push(outline);
            }
        }

        let overall = overall.unwrap();
        let text_w = (overall.max.x - overall.min.x).ceil() as u32;
        let text_h = (overall.max.y - overall.min.y).ceil() as u32;

        let offset_x = (buf_w as i32 - text_w as i32) / 2;
        let offset_y = (buf_h as i32 - text_h as i32) / 2;

        let mut buf = vec![0u8; (buf_w * buf_h) as usize];

        let min_x = overall.min.x;
        let min_y = overall.min.y;
        let buf_ptr = buf.as_mut_ptr();
        let buf_len = buf.len() as u32;
        for outline in outlines {
            let origin = outline.px_bounds().min;
            outline.draw(|x, y, coverage| {
                let ax = x as f32 + origin.x;
                let ay = y as f32 + origin.y;
                let px = (ax as i32 - min_x as i32) + offset_x;
                let py = (buf_h as i32 - 1) - ((ay as i32 - min_y as i32) + offset_y);
                if px >= 0 && px < buf_w as i32 && py >= 0 && py < buf_h as i32 {
                    let idx = (px as u32 + py as u32 * buf_w) as usize;
                    if idx < buf_len as usize {
                        unsafe { *buf_ptr.add(idx) = (coverage * 255.0) as u8; }
                    }
                }
            });
        }

        unsafe {
            let tex = gl.create_texture().expect("Cannot create texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                buf_w as i32,
                buf_h as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&buf)),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            tex
        }
    }

    fn create_fbo(
        gl: &glow::Context,
        w: u32,
        h: u32,
    ) -> (glow::Framebuffer, glow::Texture) {
        use glow::HasContext as _;
        unsafe {
            let tex = gl.create_texture().expect("Cannot create FBO texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w as i32,
                h as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            let fbo = gl.create_framebuffer().expect("Cannot create FBO");
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);

            (fbo, tex)
        }
    }

    fn compile_program(
        gl: &glow::Context,
        shader_version: &str,
        vs_source: &str,
        fs_source: &str,
    ) -> glow::Program {
        use glow::HasContext as _;
        unsafe {
            let program = gl.create_program().expect("Cannot create program");
            let shaders = [
                (glow::VERTEX_SHADER, vs_source),
                (glow::FRAGMENT_SHADER, fs_source),
            ];

            let compiled: Vec<_> = shaders
                .iter()
                .map(|(shader_type, src)| {
                    let shader = gl.create_shader(*shader_type).expect("Cannot create shader");
                    gl.shader_source(shader, &format!("{shader_version}\n{src}"));
                    gl.compile_shader(shader);
                    assert!(
                        gl.get_shader_compile_status(shader),
                        "Failed to compile {shader_type}: {}",
                        gl.get_shader_info_log(shader)
                    );
                    gl.attach_shader(program, shader);
                    shader
                })
                .collect();

            gl.link_program(program);
            assert!(
                gl.get_program_link_status(program),
                "{}",
                gl.get_program_info_log(program)
            );

            for s in compiled {
                gl.detach_shader(program, s);
                gl.delete_shader(s);
            }

            program
        }
    }

    fn destroy(&self, gl: &glow::Context) {
        use glow::HasContext as _;
        unsafe {
            gl.delete_program(self.prog_copy);
            gl.delete_program(self.prog_sharp);
            gl.delete_program(self.prog_blur_h);
            gl.delete_program(self.prog_blur_v);
            gl.delete_program(self.prog_circle);
            gl.delete_texture(self.text_tex);
            gl.delete_texture(self.tex_a);
            gl.delete_texture(self.tex_b);
            gl.delete_framebuffer(self.fbo_a);
            gl.delete_framebuffer(self.fbo_b);
            gl.delete_vertex_array(self.vao);
        }
    }

    fn paint(&self, gl: &glow::Context, _rect: egui::Rect) {
        use glow::HasContext as _;
        unsafe {
            let mut vp = [0i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);

            // --- Render text to FBO A ---
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo_a));
            gl.viewport(0, 0, self.width as i32, self.height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.use_program(Some(self.prog_copy));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.text_tex));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_copy, "u_tex").as_ref(),
                0,
            );
            gl.uniform_1_f32(
                gl.get_uniform_location(self.prog_copy, "u_intensity").as_ref(),
                1.0,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // --- Blur horizontal: FBO A → FBO B ---
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo_b));
            gl.viewport(0, 0, self.width as i32, self.height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.use_program(Some(self.prog_blur_h));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex_a));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_blur_h, "u_tex").as_ref(),
                0,
            );
            let texel_h = 1.0 / self.width as f32;
            gl.uniform_2_f32(
                gl.get_uniform_location(self.prog_blur_h, "u_texel").as_ref(),
                texel_h,
                0.0,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // --- Blur vertical: FBO B → FBO A ---
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo_a));
            gl.viewport(0, 0, self.width as i32, self.height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.use_program(Some(self.prog_blur_v));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex_b));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_blur_v, "u_tex").as_ref(),
                0,
            );
            let texel_v = 1.0 / self.height as f32;
            gl.uniform_2_f32(
                gl.get_uniform_location(self.prog_blur_v, "u_texel").as_ref(),
                0.0,
                texel_v,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // --- Render to default framebuffer ---
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.viewport(vp[0], vp[1], vp[2], vp[3]);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            // Bloom (additive blending)
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE);

            gl.use_program(Some(self.prog_copy));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex_a));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_copy, "u_tex").as_ref(),
                0,
            );
            gl.uniform_1_f32(
                gl.get_uniform_location(self.prog_copy, "u_intensity").as_ref(),
                1.8,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // Sharp text (alpha blending)
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.prog_sharp));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.text_tex));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_sharp, "u_tex").as_ref(),
                0,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            gl.disable(glow::BLEND);
        }
    }
}
