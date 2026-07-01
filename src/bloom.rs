#![expect(unsafe_code)]
#![expect(clippy::undocumented_unsafe_blocks)]

use eframe::{egui, egui_glow, glow};
use egui::mutex::Mutex;
use std::sync::Arc;

pub struct BloomRenderer {
    text_tex: Option<glow::Texture>,
    fbo_bloom_a: Option<glow::Framebuffer>,
    tex_bloom_a: Option<glow::Texture>,
    fbo_bloom_b: Option<glow::Framebuffer>,
    tex_bloom_b: Option<glow::Texture>,
    prog_copy: glow::Program,
    prog_sharp: glow::Program,
    prog_blur_h: glow::Program,
    prog_blur_v: glow::Program,
    vao: glow::VertexArray,
    half_w: u32,
    half_h: u32,
    font_data: Vec<u8>,
}

impl BloomRenderer {
    pub fn new(gl: &glow::Context, font_data: Vec<u8>) -> Self {
        use glow::HasContext as _;

        let shader_version = if cfg!(target_arch = "wasm32") {
            "#version 300 es"
        } else {
            "#version 330"
        };

        unsafe {
            let vao = gl.create_vertex_array().expect("Cannot create VAO");

            let prog_copy = Self::compile_program(
                gl,
                shader_version,
                "out vec2 v_uv;
                 void main() {
                     const vec2 verts[3] = vec2[3](
                         vec2(-1.0, -1.0),
                         vec2(3.0, -1.0),
                         vec2(-1.0, 3.0)
                     );
                     v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                     gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                 }",
                "precision mediump float;
                 in vec2 v_uv;
                 uniform sampler2D u_tex;
                 uniform float u_intensity;
                 out vec4 out_color;
                 void main() {
                     float v = texture(u_tex, v_uv).r;
                     out_color = vec4(vec3(v * u_intensity), 1.0);
                 }",
            );

            let prog_sharp = Self::compile_program(
                gl,
                shader_version,
                "out vec2 v_uv;
                 void main() {
                     const vec2 verts[3] = vec2[3](
                         vec2(-1.0, -1.0),
                         vec2(3.0, -1.0),
                         vec2(-1.0, 3.0)
                     );
                     v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                     gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                 }",
                "precision mediump float;
                 in vec2 v_uv;
                 uniform sampler2D u_tex;
                 out vec4 out_color;
                 void main() {
                     float v = texture(u_tex, v_uv).r;
                     out_color = vec4(vec3(v), v);
                 }",
            );

            let prog_blur_h = Self::compile_program(
                gl,
                shader_version,
                "out vec2 v_uv;
                 void main() {
                     const vec2 verts[3] = vec2[3](
                         vec2(-1.0, -1.0),
                         vec2(3.0, -1.0),
                         vec2(-1.0, 3.0)
                     );
                     v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                     gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                 }",
                "precision mediump float;
                 in vec2 v_uv;
                 uniform sampler2D u_tex;
                 uniform vec2 u_texel;
                 out vec4 out_color;
                 void main() {
                     vec4 c = texture(u_tex, v_uv) * 0.2270270270;
                     c += texture(u_tex, v_uv + vec2(u_texel.x * 1.3846153846, 0.0)) * 0.3162162162;
                     c += texture(u_tex, v_uv - vec2(u_texel.x * 1.3846153846, 0.0)) * 0.3162162162;
                     c += texture(u_tex, v_uv + vec2(u_texel.x * 3.2307692308, 0.0)) * 0.0702702703;
                     c += texture(u_tex, v_uv - vec2(u_texel.x * 3.2307692308, 0.0)) * 0.0702702703;
                     out_color = c;
                 }",
            );

            let prog_blur_v = Self::compile_program(
                gl,
                shader_version,
                "out vec2 v_uv;
                 void main() {
                     const vec2 verts[3] = vec2[3](
                         vec2(-1.0, -1.0),
                         vec2(3.0, -1.0),
                         vec2(-1.0, 3.0)
                     );
                     v_uv = verts[gl_VertexID] * 0.5 + 0.5;
                     gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                 }",
                "precision mediump float;
                 in vec2 v_uv;
                 uniform sampler2D u_tex;
                 uniform vec2 u_texel;
                 out vec4 out_color;
                 void main() {
                     vec4 c = texture(u_tex, v_uv) * 0.2270270270;
                     c += texture(u_tex, v_uv + vec2(0.0, u_texel.y * 1.3846153846)) * 0.3162162162;
                     c += texture(u_tex, v_uv - vec2(0.0, u_texel.y * 1.3846153846)) * 0.3162162162;
                     c += texture(u_tex, v_uv + vec2(0.0, u_texel.y * 3.2307692308)) * 0.0702702703;
                     c += texture(u_tex, v_uv - vec2(0.0, u_texel.y * 3.2307692308)) * 0.0702702703;
                     out_color = c;
                 }",
            );

            Self {
                text_tex: None,
                fbo_bloom_a: None,
                tex_bloom_a: None,
                fbo_bloom_b: None,
                tex_bloom_b: None,
                prog_copy,
                prog_sharp,
                prog_blur_h,
                prog_blur_v,
                vao,
                half_w: 0,
                half_h: 0,
                font_data,
            }
        }
    }

    pub fn render_text(
        &mut self,
        gl: &glow::Context,
        text: &str,
        intensity: f32,
        font_scale: f32,
        win_w: u32,
        win_h: u32,
    ) {
        if win_w == 0 || win_h == 0 {
            return;
        }
        use glow::HasContext as _;

        if let Some(tex) = self.text_tex.take() {
            unsafe {
                gl.delete_texture(tex);
            }
        }

        let text_tex = self.create_text_texture(gl, text, font_scale, win_w, win_h);
        self.text_tex = Some(text_tex);

        let half_w = (win_w / 4).max(1);
        let half_h = (win_h / 4).max(1);
        self.ensure_fbos(gl, half_w, half_h);

        let text_tex = self.text_tex.unwrap();
        let fbo_a = self.fbo_bloom_a.unwrap();
        let fbo_b = self.fbo_bloom_b.unwrap();
        let tex_a = self.tex_bloom_a.unwrap();
        let tex_b = self.tex_bloom_b.unwrap();

        unsafe {
            let mut vp = [0i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo_a));
            gl.viewport(0, 0, self.half_w as i32, self.half_h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(self.prog_copy));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(text_tex));
            gl.uniform_1_i32(gl.get_uniform_location(self.prog_copy, "u_tex").as_ref(), 0);
            gl.uniform_1_f32(
                gl.get_uniform_location(self.prog_copy, "u_intensity")
                    .as_ref(),
                1.0,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo_b));
            gl.viewport(0, 0, self.half_w as i32, self.half_h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(self.prog_blur_h));
            gl.bind_texture(glow::TEXTURE_2D, Some(tex_a));
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

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo_a));
            gl.viewport(0, 0, self.half_w as i32, self.half_h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(self.prog_blur_v));
            gl.bind_texture(glow::TEXTURE_2D, Some(tex_b));
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

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.viewport(vp[0], vp[1], vp[2], vp[3]);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.enable(glow::BLEND);

            gl.blend_func(glow::ONE, glow::ONE);
            gl.use_program(Some(self.prog_copy));
            gl.bind_texture(glow::TEXTURE_2D, Some(tex_a));
            gl.uniform_1_i32(gl.get_uniform_location(self.prog_copy, "u_tex").as_ref(), 0);
            gl.uniform_1_f32(
                gl.get_uniform_location(self.prog_copy, "u_intensity")
                    .as_ref(),
                intensity,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.prog_sharp));
            gl.bind_texture(glow::TEXTURE_2D, Some(text_tex));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.prog_sharp, "u_tex").as_ref(),
                0,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);

            gl.disable(glow::BLEND);
        }
    }

    pub fn destroy(&mut self, gl: &glow::Context) {
        use glow::HasContext as _;
        unsafe {
            gl.delete_program(self.prog_copy);
            gl.delete_program(self.prog_sharp);
            gl.delete_program(self.prog_blur_h);
            gl.delete_program(self.prog_blur_v);
            if let Some(tex) = self.text_tex.take() {
                gl.delete_texture(tex);
            }
            if let Some(tex) = self.tex_bloom_a.take() {
                gl.delete_texture(tex);
            }
            if let Some(tex) = self.tex_bloom_b.take() {
                gl.delete_texture(tex);
            }
            if let Some(fbo) = self.fbo_bloom_a.take() {
                gl.delete_framebuffer(fbo);
            }
            if let Some(fbo) = self.fbo_bloom_b.take() {
                gl.delete_framebuffer(fbo);
            }
            gl.delete_vertex_array(self.vao);
        }
    }

    fn create_text_texture(
        &self,
        gl: &glow::Context,
        text: &str,
        font_scale: f32,
        win_w: u32,
        win_h: u32,
    ) -> glow::Texture {
        use ab_glyph::{Font as _, ScaleFont as _, point};
        use glow::HasContext as _;

        let font =
            ab_glyph::FontArc::try_from_vec(self.font_data.clone()).expect("Invalid font");

        let compute = |scale_px: f32| -> Option<(Vec<ab_glyph::OutlinedGlyph>, ab_glyph::Rect)> {
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
            overall.map(|o| (outlines, o))
        };

        let padding = 10.0f32;
        let max_text_w = win_w as f32 - padding * 2.0;
        let Some((outlines, overall)) = compute(font_scale) else {
            return self.create_empty_texture(gl, win_w, win_h);
        };
        let text_w = (overall.max.x - overall.min.x).ceil() as u32;

        let scale = if text_w as f32 > max_text_w {
            max_text_w / text_w as f32
        } else {
            1.0
        };

        let (outlines, overall) = if scale < 1.0 {
            match compute(font_scale * scale) {
                Some(r) => r,
                None => return self.create_empty_texture(gl, win_w, win_h),
            }
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
                        unsafe {
                            *buf_ptr.add(idx) = (coverage * 255.0) as u8;
                        }
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
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
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

    fn create_empty_texture(&self, gl: &glow::Context, win_w: u32, win_h: u32) -> glow::Texture {
        use glow::HasContext as _;
        let buf = vec![0u8; (win_w * win_h) as usize];
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
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
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

    fn ensure_fbos(&mut self, gl: &glow::Context, half_w: u32, half_h: u32) {
        if self.half_w == half_w && self.half_h == half_h && self.fbo_bloom_a.is_some() {
            return;
        }
        use glow::HasContext as _;

        if let Some(fbo) = self.fbo_bloom_a.take() {
            unsafe { gl.delete_framebuffer(fbo) };
        }
        if let Some(tex) = self.tex_bloom_a.take() {
            unsafe { gl.delete_texture(tex) };
        }
        if let Some(fbo) = self.fbo_bloom_b.take() {
            unsafe { gl.delete_framebuffer(fbo) };
        }
        if let Some(tex) = self.tex_bloom_b.take() {
            unsafe { gl.delete_texture(tex) };
        }

        unsafe {
            let (fbo_a, tex_a) = Self::create_fbo_r16f(gl, half_w, half_h);
            let (fbo_b, tex_b) = Self::create_fbo_r16f(gl, half_w, half_h);
            self.fbo_bloom_a = Some(fbo_a);
            self.tex_bloom_a = Some(tex_a);
            self.fbo_bloom_b = Some(fbo_b);
            self.tex_bloom_b = Some(tex_b);
            self.half_w = half_w;
            self.half_h = half_h;
        }
    }

    #[expect(unsafe_op_in_unsafe_fn)]
    unsafe fn create_fbo_r16f(
        gl: &glow::Context,
        w: u32,
        h: u32,
    ) -> (glow::Framebuffer, glow::Texture) {
        use glow::HasContext as _;
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
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
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
                    let shader = gl
                        .create_shader(*shader_type)
                        .expect("Cannot create shader");
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
}

pub struct BloomText {
    renderer: Arc<Mutex<BloomRenderer>>,
    text: String,
    intensity: f32,
    font_scale: f32,
}

impl BloomText {
    pub fn new(renderer: Arc<Mutex<BloomRenderer>>, text: impl Into<String>) -> Self {
        Self {
            renderer,
            text: text.into(),
            intensity: 1.8,
            font_scale: 72.0,
        }
    }

    pub fn intensity(mut self, value: f32) -> Self {
        self.intensity = value;
        self
    }

    pub fn font_scale(mut self, value: f32) -> Self {
        self.font_scale = value;
        self
    }
}

impl egui::Widget for BloomText {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.ctx().request_repaint();

        let rect = ui.available_rect_before_wrap();
        let win_w = rect.width().ceil() as u32;
        let win_h = rect.height().ceil() as u32;

        if win_w == 0 || win_h == 0 {
            return ui.allocate_response(rect.size(), egui::Sense::hover());
        }

        let renderer = Arc::clone(&self.renderer);
        let text = self.text;
        let intensity = self.intensity;
        let font_scale = self.font_scale;

        let callback = egui::PaintCallback {
            rect,
            callback: std::sync::Arc::new(egui_glow::CallbackFn::new(
                move |_info, painter| {
                    let gl = painter.gl();
                    let mut r = renderer.lock();
                    r.render_text(gl, &text, intensity, font_scale, win_w, win_h);
                },
            )),
        };
        ui.painter().add(callback);
        ui.allocate_response(rect.size(), egui::Sense::hover())
    }
}
