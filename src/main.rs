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
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 1024.0]),
        multisampling: 4,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Glow",
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
    fbo_bloom_a: glow::Framebuffer,
    tex_bloom_a: glow::Texture,
    fbo_bloom_b: glow::Framebuffer,
    tex_bloom_b: glow::Texture,
    prog_copy: glow::Program,
    prog_sharp: glow::Program,
    prog_blur_h: glow::Program,
    prog_blur_v: glow::Program,
    vao: glow::VertexArray,
    half_w: u32,
    half_h: u32,
}

impl BloomEffect {
    fn new(gl: &glow::Context) -> Self {
        use glow::HasContext as _;

        let width = 1024;
        let height = 1024;
        let half_w = width/2;
        let half_h = height/2;

        let shader_version = if cfg!(target_arch = "wasm32") {
            "#version 300 es"
        } else {
            "#version 330"
        };

        unsafe {
            let text_tex = Self::create_text_texture(gl, width, height);
            let vao = gl.create_vertex_array().expect("Cannot create VAO");
            let (fbo_bloom_a, tex_bloom_a) = Self::create_fbo_r16f(gl, half_w, half_h);
            let (fbo_bloom_b, tex_bloom_b) = Self::create_fbo_r16f(gl, half_w, half_h);

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

            Self {
                text_tex,
                fbo_bloom_a,
                tex_bloom_a,
                fbo_bloom_b,
                tex_bloom_b,
                prog_copy,
                prog_sharp,
                prog_blur_h,
                prog_blur_v,
                vao,
                half_w,
                half_h,
            }
        }
    }

    fn create_text_texture(gl: &glow::Context, win_w: u32, win_h: u32) -> glow::Texture {
        use glow::HasContext as _;
        use ab_glyph::{point, Font as _, ScaleFont as _};

        let text = "Encrypted virtual file system";
        let font_path = "C:\\Windows\\Fonts\\arial.ttf";

        let font_data = std::fs::read(font_path).expect("Cannot read font file");
        let font = ab_glyph::FontArc::try_from_vec(font_data).expect("Invalid font");

        let compute = |scale_px: f32| -> (Vec<ab_glyph::OutlinedGlyph>, ab_glyph::Rect) {
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
            (outlines, overall.unwrap())
        };

        let padding = 10.0f32;
        let max_text_w = win_w as f32 - padding * 2.0;
        let (outlines, overall) = compute(72.0);
        let text_w = (overall.max.x - overall.min.x).ceil() as u32;
        let _text_h = (overall.max.y - overall.min.y).ceil() as u32;

        let scale = if text_w as f32 > max_text_w {
            max_text_w / text_w as f32
        } else {
            1.0
        };

        let (outlines, overall) = if scale < 1.0 {
            compute(72.0 * scale)
        } else {
            (outlines, overall)
        };

        let text_w = (overall.max.x - overall.min.x).ceil() as u32;
        let text_h = (overall.max.y - overall.min.y).ceil() as u32;

        let offset_x = (win_w as i32 - text_w as i32) / 2;
        let offset_y = (win_h as i32 - text_h as i32) / 2;

        let mut buf = vec![0u8; (win_w * win_h) as usize];

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
                let py = (win_h as i32 - 1) - ((ay as i32 - min_y as i32) + offset_y);
                if px >= 0 && px < win_w as i32 && py >= 0 && py < win_h as i32 {
                    let idx = (px as u32 + py as u32 * win_w) as usize;
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
                win_w as i32,
                win_h as i32,
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

    fn create_fbo_r16f(
        gl: &glow::Context,
        w: u32,
        h: u32,
    ) -> (glow::Framebuffer, glow::Texture) {
        use glow::HasContext as _;
        unsafe {
            let tex = gl.create_texture().expect("Cannot create R16F texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R16F as i32,
                w as i32,
                h as i32,
                0,
                glow::RED,
                glow::FLOAT,
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
            gl.delete_texture(self.text_tex);
            gl.delete_texture(self.tex_bloom_a);
            gl.delete_texture(self.tex_bloom_b);
            gl.delete_framebuffer(self.fbo_bloom_a);
            gl.delete_framebuffer(self.fbo_bloom_b);
            gl.delete_vertex_array(self.vao);
        }
    }

    fn paint(&self, gl: &glow::Context, _rect: egui::Rect) {
        use glow::HasContext as _;
        unsafe {
            let mut vp = [0i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);

            // Step 1: Copy text_tex → fbo_bloom_a (full → half downsample)
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo_bloom_a));
            gl.viewport(0, 0, self.half_w as i32, self.half_h as i32);
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
                gl.get_uniform_location(self.prog_copy, "u_intensity")
                    .as_ref(),
                1.0,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // Step 2: Blur horizontal: fbo_bloom_a → fbo_bloom_b
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo_bloom_b));
            gl.viewport(0, 0, self.half_w as i32, self.half_h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(self.prog_blur_h));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex_bloom_a));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_blur_h, "u_tex").as_ref(),
                0,
            );
            gl.uniform_2_f32(
                gl.get_uniform_location(self.prog_blur_h, "u_texel")
                    .as_ref(),
                1.0 / self.half_w as f32,
                1.0 / self.half_h as f32,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // Step 3: Blur vertical: fbo_bloom_b → fbo_bloom_a
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo_bloom_a));
            gl.viewport(0, 0, self.half_w as i32, self.half_h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(self.prog_blur_v));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex_bloom_b));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_blur_v, "u_tex").as_ref(),
                0,
            );
            gl.uniform_2_f32(
                gl.get_uniform_location(self.prog_blur_v, "u_texel")
                    .as_ref(),
                1.0 / self.half_w as f32,
                1.0 / self.half_h as f32,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // Step 4: Render to screen
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.viewport(vp[0], vp[1], vp[2], vp[3]);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.enable(glow::BLEND);

            // Bloom: fbo_bloom_a → screen (half → full), additive
            gl.blend_func(glow::ONE, glow::ONE);
            gl.use_program(Some(self.prog_copy));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex_bloom_a));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_copy, "u_tex").as_ref(),
                0,
            );
            gl.uniform_1_f32(
                gl.get_uniform_location(self.prog_copy, "u_intensity")
                    .as_ref(),
                1.8,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            // Sharp text: text_tex → screen, alpha blending
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
