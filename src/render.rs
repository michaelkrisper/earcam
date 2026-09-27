//! Two passes, each a single full-screen triangle, once per camera frame:
//!
//! 1. At the camera's resolution, into an offscreen texture: deblocking. The JPEGs are
//!    compressed hard (~0.6 bit/pixel), which leaves a visible 8x8 grid in luma and a
//!    16x16 one in chroma. Like H.264's in-loop filter, this only touches pixels next to a
//!    block boundary where both sides are flat and the step is small, i.e. an artifact
//!    rather than a real edge, and spreads the step into a ramp.
//! 2. Rotation and circular mask, plain bilinear sampling. Anything sharper than bilinear
//!    was tried on the Android app and turns the block grid into a mosaic.
//!
//! The mask hides the corners of the square image, which would otherwise sweep around and
//! betray every bit of rotation. It is a little wider than the window, so the visible edge
//! reads as a frame rather than a porthole.

use glow::{
    HasContext, NativeFramebuffer, NativeProgram, NativeTexture, NativeUniformLocation,
    PixelUnpackData,
};

use crate::stream::Slot;

const MASK_OVERSCAN: f32 = 1.1;

pub struct Renderer {
    gl: glow::Context,
    camera_tex: NativeTexture,
    deblocked_tex: NativeTexture,
    fbo: NativeFramebuffer,
    tex_size: (usize, usize),
    deblock: NativeProgram,
    display: NativeProgram,
    u_center: Option<NativeUniformLocation>,
    u_radius: Option<NativeUniformLocation>,
    u_rotation: Option<NativeUniformLocation>,
}

impl Renderer {
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        unsafe {
            // The vertex shader needs no attributes, but a bound VAO keeps every driver happy.
            let vao = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(vao));
            let version = shader_version(gl.version().is_embedded);
            let deblock = program(&gl, version, VERTEX_SHADER, DEBLOCK_SHADER)?;
            let display = program(&gl, version, VERTEX_SHADER, DISPLAY_SHADER)?;
            let camera_tex = texture(&gl)?;
            let deblocked_tex = texture(&gl)?;
            let fbo = gl.create_framebuffer()?;
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            Ok(Renderer {
                u_center: gl.get_uniform_location(display, "uCenter"),
                u_radius: gl.get_uniform_location(display, "uRadius"),
                u_rotation: gl.get_uniform_location(display, "uRotation"),
                gl,
                camera_tex,
                deblocked_tex,
                fbo,
                tex_size: (0, 0),
                deblock,
                display,
            })
        }
    }

    /// Uploads a camera frame and deblocks it into the display texture.
    pub fn upload(&mut self, frame: &Slot) {
        let (w, h) = (frame.width as i32, frame.height as i32);
        if w == 0 || h == 0 {
            return;
        }
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.camera_tex));
            let pixels = PixelUnpackData::Slice(Some(&frame.rgb));
            if self.tex_size == (frame.width, frame.height) {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w,
                    h,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    pixels,
                );
            } else {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGB8 as i32,
                    w,
                    h,
                    0,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    pixels,
                );
                gl.bind_texture(glow::TEXTURE_2D, Some(self.deblocked_tex));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    w,
                    h,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    PixelUnpackData::Slice(None),
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(self.deblocked_tex),
                    0,
                );
                gl.bind_texture(glow::TEXTURE_2D, Some(self.camera_tex));
                self.tex_size = (frame.width, frame.height);
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.viewport(0, 0, w, h);
            gl.use_program(Some(self.deblock));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }

    /// Draws the deblocked frame turned clockwise by `angle` degrees.
    pub fn draw(&self, width: u32, height: u32, angle: f32) {
        let gl = &self.gl;
        let (w, h) = (width as f32, height as f32);
        let rad = angle.to_radians();
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear(glow::COLOR_BUFFER_BIT);
            if self.tex_size.0 == 0 {
                return;
            }
            gl.use_program(Some(self.display));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.deblocked_tex));
            gl.uniform_2_f32(self.u_center.as_ref(), w / 2.0, h / 2.0);
            gl.uniform_1_f32(self.u_radius.as_ref(), w.min(h) / 2.0 * MASK_OVERSCAN);
            gl.uniform_2_f32(self.u_rotation.as_ref(), rad.cos(), rad.sin());
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }
}

unsafe fn texture(gl: &glow::Context) -> Result<NativeTexture, String> {
    unsafe {
        let t = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
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
        Ok(t)
    }
}

/// The shaders are GLSL ES 3.00 as on Android. The same source compiles as GLSL 3.30 on a
/// desktop GL context (Windows without GLES support), where precision qualifiers are
/// accepted and ignored.
pub fn shader_version(embedded: bool) -> &'static str {
    if embedded {
        "#version 300 es\n"
    } else {
        "#version 330 core\n"
    }
}

unsafe fn program(
    gl: &glow::Context,
    version: &str,
    vertex: &str,
    fragment: &str,
) -> Result<NativeProgram, String> {
    unsafe {
        let program = gl.create_program()?;
        for (ty, src) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl.create_shader(ty)?;
            gl.shader_source(shader, &format!("{version}{src}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(format!("shader: {}", gl.get_shader_info_log(shader)));
            }
            gl.attach_shader(program, shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        Ok(program)
    }
}

/// One triangle covering the viewport, generated from the vertex ID: no buffers.
const VERTEX_SHADER: &str = r"void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

/// ALPHA: largest step across a block boundary still treated as an artifact.
/// BETA: largest step within either side for that side to count as flat.
/// Luma works on the 8x8 grid, chroma (_C, in YCbCr) on the 16x16 one.
/// Tuned on hardware in the Android app; keep the two in sync.
const DEBLOCK_SHADER: &str = r"precision highp float;
uniform sampler2D uFrame;
out vec4 color;
const float ALPHA = 0.10;
const float BETA = 0.04;
const float ALPHA_C = 0.06;
const float BETA_C = 0.03;
const vec3 LUMA = vec3(0.299, 0.587, 0.114);

vec3 at(ivec2 p) {
    return texelFetch(uFrame, clamp(p, ivec2(0), textureSize(uFrame, 0) - 1), 0).rgb;
}

// JPEG's full-range BT.601 Cb, Cr.
vec2 chroma(ivec2 p) {
    vec3 c = at(p);
    return vec2(dot(c, vec3(-0.168736, -0.331264, 0.5)), dot(c, vec3(0.5, -0.418688, -0.081312)));
}

bool border(ivec2 q0, ivec2 dir) {
    return q0.x * dir.x + q0.y * dir.y == 0;
}

// Luma correction for the pixel at p from the 8x8 block boundary along dir, where b is
// p's position within its block along dir. The boundary lies between p0 and q0; p1, p0,
// q0, q1 move by +1/8, +3/8, -3/8, -1/8 of the step, turning it into a ramp.
vec3 deblock(ivec2 p, int b, ivec2 dir) {
    ivec2 q0;
    float w;
    if (b == 0)      { q0 = p;           w = -3.0 / 8.0; }
    else if (b == 1) { q0 = p - dir;     w = -1.0 / 8.0; }
    else if (b == 7) { q0 = p + dir;     w =  3.0 / 8.0; }
    else if (b == 6) { q0 = p + 2 * dir; w =  1.0 / 8.0; }
    else return vec3(0.0);
    if (border(q0, dir)) return vec3(0.0);
    vec3 cp1 = at(q0 - 2 * dir), cp0 = at(q0 - dir), cq0 = at(q0), cq1 = at(q0 + dir);
    float p1 = dot(cp1, LUMA), p0 = dot(cp0, LUMA), q0l = dot(cq0, LUMA), q1 = dot(cq1, LUMA);
    float edge = 1.0 - smoothstep(0.6 * ALPHA, ALPHA, abs(q0l - p0));
    float even = 1.0 - smoothstep(0.6 * BETA, BETA, max(abs(p1 - p0), abs(q1 - q0l)));
    return (cq0 - cp0) * w * edge * even;
}

// Cb/Cr correction from the 16x16 chroma block boundary along dir, b being p's position
// within its 16-pixel block. A chroma sample spans two pixels, so each side is read as
// two samples of two pixels, and the step is ramped over four pixels either side.
vec2 deblockChroma(ivec2 p, int b, ivec2 dir) {
    int t = b < 8 ? b : b - 16;                 // signed offset from the nearest boundary
    if (t < -4 || t > 3) return vec2(0.0);
    ivec2 q0 = p - t * dir;
    if (border(q0, dir)) return vec2(0.0);
    vec2 p1 = 0.5 * (chroma(q0 - 4 * dir) + chroma(q0 - 3 * dir));
    vec2 p0 = 0.5 * (chroma(q0 - 2 * dir) + chroma(q0 - dir));
    vec2 q0c = 0.5 * (chroma(q0) + chroma(q0 + dir));
    vec2 q1 = 0.5 * (chroma(q0 + 2 * dir) + chroma(q0 + 3 * dir));
    vec2 step = q0c - p0;
    float edge = 1.0 - smoothstep(0.6 * ALPHA_C, ALPHA_C, length(step));
    float even = 1.0 - smoothstep(0.6 * BETA_C, BETA_C, max(length(p1 - p0), length(q1 - q0c)));
    // Where the pixel should sit on a straight ramp from -step/2 to +step/2 over 8 pixels.
    float w = (float(t) + 0.5) / 8.0 + (t < 0 ? 0.5 : -0.5);
    return step * w * edge * even;
}

void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    vec3 c = at(p) + deblock(p, p.x & 7, ivec2(1, 0)) + deblock(p, p.y & 7, ivec2(0, 1));
    vec2 d = deblockChroma(p, p.x & 15, ivec2(1, 0)) + deblockChroma(p, p.y & 15, ivec2(0, 1));
    c += vec3(1.402 * d.y, -0.344136 * d.x - 0.714136 * d.y, 1.772 * d.x);
    color = vec4(clamp(c, 0.0, 1.0), 1.0);
}
";

const DISPLAY_SHADER: &str = r"precision highp float;
uniform sampler2D uTex;
uniform vec2 uCenter;
uniform float uRadius;
uniform vec2 uRotation; // cos, sin of the clockwise image rotation
out vec4 color;
void main() {
    // Offset from the circle centre, y pointing down like the image rows.
    vec2 p = vec2(gl_FragCoord.x - uCenter.x, uCenter.y - gl_FragCoord.y);
    float inside = clamp(uRadius - length(p) + 0.5, 0.0, 1.0);
    // Undo the image rotation to find where this pixel samples it.
    vec2 q = vec2(uRotation.x * p.x + uRotation.y * p.y, uRotation.x * p.y - uRotation.y * p.x);
    vec2 uv = q / (2.0 * uRadius) + 0.5;
    color = vec4(texture(uTex, uv).rgb * inside, 1.0);
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaders_leave_the_version_to_the_context() {
        for src in [VERTEX_SHADER, DEBLOCK_SHADER, DISPLAY_SHADER] {
            assert!(!src.contains("#version"));
        }
        assert!(shader_version(true).starts_with("#version 300 es"));
        assert!(shader_version(false).starts_with("#version 330 core"));
    }
}
