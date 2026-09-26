package com.krisper.earcam

import android.content.Context
import android.graphics.Bitmap
import android.opengl.GLES30.*
import android.opengl.GLSurfaceView
import android.opengl.GLUtils
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.Surface
import android.view.SurfaceHolder
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.min
import kotlin.math.sin

/**
 * The whole UI: the camera image, rotated upright inside a circle on black.
 *
 * Frames go from the decoder thread straight to the GL thread through [exchange]; the
 * UI thread never sees them. It draws exactly once per camera frame, and tells the
 * system so, which lets the display drop to its lowest refresh rate. The one exception
 * is a pinch or drag in progress, which redraws at touch rate so zooming does not lag.
 *
 * Pinch zooms the whole picture, circle included, around the fingers; one finger pans,
 * double tap resets.
 */
class CameraView(context: Context) : GLSurfaceView(context) {

    val exchange = FrameExchange(::requestRender)

    /** Magnification and the circle centre's offset from the screen centre, in pixels. */
    private data class Zoom(val scale: Float = 1f, val panX: Float = 0f, val panY: Float = 0f)

    @Volatile private var zoom = Zoom()

    private val renderer = Renderer(exchange) { zoom }

    private val scaleDetector = ScaleGestureDetector(
        context,
        object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
            override fun onScale(detector: ScaleGestureDetector): Boolean {
                zoomAround(detector.focusX, detector.focusY, zoom.scale * detector.scaleFactor)
                return true
            }
        },
    )

    private val gestureDetector = GestureDetector(
        context,
        object : GestureDetector.SimpleOnGestureListener() {
            override fun onScroll(e1: MotionEvent?, e2: MotionEvent, dx: Float, dy: Float): Boolean {
                // dx/dy are the reverse of the finger's motion; the picture follows the finger.
                update(zoom.let { it.copy(panX = it.panX - dx, panY = it.panY - dy) })
                return true
            }

            override fun onDoubleTap(e: MotionEvent): Boolean {
                update(Zoom())
                return true
            }
        },
    )

    init {
        setEGLContextClientVersion(3)
        setEGLConfigChooser(8, 8, 8, 0, 0, 0)
        preserveEGLContextOnPause = true
        setRenderer(renderer)
        renderMode = RENDERMODE_WHEN_DIRTY
        keepScreenOn = true
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        scaleDetector.onTouchEvent(event)
        gestureDetector.onTouchEvent(event)
        return true
    }

    /** Zooms so that the point under ([fx], [fy]) stays under the fingers. */
    private fun zoomAround(fx: Float, fy: Float, scale: Float) {
        val z = zoom
        val next = scale.coerceIn(1f, MAX_ZOOM)
        val f = next / z.scale
        val cx = fx - width / 2f
        val cy = fy - height / 2f
        update(Zoom(next, cx - (cx - z.panX) * f, cy - (cy - z.panY) * f))
    }

    /** Keeps the screen centre on the magnified circle, then redraws. */
    private fun update(z: Zoom) {
        val max = min(width, height) / 2f * MASK_OVERSCAN * (z.scale - 1f)
        val len = hypot(z.panX, z.panY)
        zoom = if (len > max) z.copy(panX = z.panX * max / len, panY = z.panY * max / len) else z
        requestRender()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        super.surfaceCreated(holder)
        holder.surface.setFrameRate(
            CAMERA_FPS,
            Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE,
            Surface.CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS,
        )
    }

    /**
     * Two passes, each a single full-screen triangle, both once per camera frame:
     *
     * 1. At the camera's 480x480, into an offscreen texture: deblocking. The camera's
     *    JPEGs are compressed hard (~0.6 bit/pixel), which leaves a visible 8x8 grid, and
     *    a coarser 16x16 one in colour, which JPEG stores at half resolution. Like the
     *    in-loop filter of H.264, this only touches pixels next to a block boundary, and
     *    only where both sides are flat and the step between them is small, i.e. where
     *    the step is a compression artifact rather than a real edge; there it spreads the
     *    step into a ramp. Block interiors and real edges pass through untouched.
     * 2. Rotation, zoom and circular mask in one fragment shader, plain bilinear sampling.
     *
     * Both deblocking stages were compared on hardware and kept. Tried and rejected: a
     * bilateral + temporal denoise, and Catmull-Rom with CAS sharpening; anything sharper
     * than bilinear turns the block grid into a mosaic.
     *
     * The mask hides the corners of the square image, which would otherwise sweep around
     * and betray every bit of rotation. It is deliberately a little wider than the screen:
     * the sides of the circle leave the viewport, so the visible edge reads as a frame
     * rather than a porthole. The image is sized so its inscribed circle covers the mask
     * at any angle.
     */
    private class Renderer(
        private val exchange: FrameExchange,
        private val zoom: () -> Zoom,
    ) : GLSurfaceView.Renderer {

        private var front = FrameSlot()
        private var width = 0
        private var height = 0

        private var cameraTex = 0
        private var deblockedTex = 0
        private var fbo = 0
        private var texWidth = 0
        private var texHeight = 0

        private var deblockProgram = 0
        private var displayProgram = 0
        private var uCenter = 0
        private var uRadius = 0
        private var uRotation = 0
        private var uZoom = 0
        private var uPan = 0

        private var angle = Orientation.UPRIGHT_OFFSET
        private var target = angle
        private var lastDraw = 0L

        override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
            deblockProgram = program(VERTEX_SHADER, DEBLOCK_SHADER)
            displayProgram = program(VERTEX_SHADER, DISPLAY_SHADER)
            glUseProgram(displayProgram)
            uCenter = glGetUniformLocation(displayProgram, "uCenter")
            uRadius = glGetUniformLocation(displayProgram, "uRadius")
            uRotation = glGetUniformLocation(displayProgram, "uRotation")
            uZoom = glGetUniformLocation(displayProgram, "uZoom")
            uPan = glGetUniformLocation(displayProgram, "uPan")

            val tex = IntArray(2)
            glGenTextures(2, tex, 0)
            cameraTex = tex[0]
            deblockedTex = tex[1]
            for (t in tex) {
                glBindTexture(GL_TEXTURE_2D, t)
                glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR)
                glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR)
                glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE)
                glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE)
            }
            val fb = IntArray(1)
            glGenFramebuffers(1, fb, 0)
            fbo = fb[0]
            glClearColor(0f, 0f, 0f, 1f)

            // A fresh context has no texture; restore the last frame instead of going black.
            texWidth = 0
            texHeight = 0
            front.bitmap?.let(::upload)
        }

        override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
            glViewport(0, 0, width, height)
            this.width = width
            this.height = height
        }

        override fun onDrawFrame(gl: GL10?) {
            exchange.take(front)?.let { slot ->
                front = slot
                slot.bitmap?.let(::upload)
                target = Orientation.UPRIGHT_OFFSET - Orientation.angleDegrees(slot.posY, slot.posZ)
            }

            val now = System.nanoTime()
            val dtMs = if (lastDraw == 0L) 0f else ((now - lastDraw) / 1e6f).coerceAtMost(100f)
            lastDraw = now
            angle = Orientation.smooth(angle, target, Orientation.factorFor(dtMs))

            glViewport(0, 0, width, height)
            glClear(GL_COLOR_BUFFER_BIT)
            if (texWidth == 0) return
            glUseProgram(displayProgram)
            glBindTexture(GL_TEXTURE_2D, deblockedTex)
            val rad = Math.toRadians(angle.toDouble())
            glUniform2f(uCenter, width / 2f, height / 2f)
            glUniform1f(uRadius, min(width, height) / 2f * MASK_OVERSCAN)
            glUniform2f(uRotation, cos(rad).toFloat(), sin(rad).toFloat())
            val z = zoom()
            glUniform1f(uZoom, z.scale)
            glUniform2f(uPan, z.panX, z.panY)
            glDrawArrays(GL_TRIANGLES, 0, 3)
        }

        /** Uploads a camera frame and deblocks it into [deblockedTex]. */
        private fun upload(bitmap: Bitmap) {
            glBindTexture(GL_TEXTURE_2D, cameraTex)
            if (bitmap.width == texWidth && bitmap.height == texHeight) {
                GLUtils.texSubImage2D(GL_TEXTURE_2D, 0, 0, 0, bitmap)
            } else {
                GLUtils.texImage2D(GL_TEXTURE_2D, 0, bitmap, 0)
                texWidth = bitmap.width
                texHeight = bitmap.height
                glBindTexture(GL_TEXTURE_2D, deblockedTex)
                glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, texWidth, texHeight, 0, GL_RGBA, GL_UNSIGNED_BYTE, null)
                glBindFramebuffer(GL_FRAMEBUFFER, fbo)
                glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, deblockedTex, 0)
                glBindTexture(GL_TEXTURE_2D, cameraTex)
            }
            glBindFramebuffer(GL_FRAMEBUFFER, fbo)
            glViewport(0, 0, texWidth, texHeight)
            glUseProgram(deblockProgram)
            glDrawArrays(GL_TRIANGLES, 0, 3)
            glBindFramebuffer(GL_FRAMEBUFFER, 0)
        }

        private fun program(vertex: String, fragment: String): Int {
            val program = glCreateProgram()
            glAttachShader(program, shader(GL_VERTEX_SHADER, vertex))
            glAttachShader(program, shader(GL_FRAGMENT_SHADER, fragment))
            glLinkProgram(program)
            val ok = IntArray(1)
            glGetProgramiv(program, GL_LINK_STATUS, ok, 0)
            check(ok[0] != 0) { glGetProgramInfoLog(program) }
            return program
        }

        private fun shader(type: Int, source: String): Int {
            val shader = glCreateShader(type)
            glShaderSource(shader, source)
            glCompileShader(shader)
            val ok = IntArray(1)
            glGetShaderiv(shader, GL_COMPILE_STATUS, ok, 0)
            check(ok[0] != 0) { glGetShaderInfoLog(shader) }
            return shader
        }
    }

    private companion object {
        const val MASK_OVERSCAN = 1.1f
        const val MAX_ZOOM = 4f

        /** What the N30 Pro streams at; the display needs nothing faster. */
        const val CAMERA_FPS = 17f

        /** One triangle covering the viewport, generated from the vertex ID: no buffers. */
        const val VERTEX_SHADER = """#version 300 es
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"""

        /**
         * ALPHA: largest step across a block boundary still treated as an artifact.
         * BETA: largest step within either side for that side to count as flat.
         * Luma works on the 8x8 grid, chroma (_C, in YCbCr) on the 16x16 one.
         */
        const val DEBLOCK_SHADER = """#version 300 es
precision highp float;
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
"""

        const val DISPLAY_SHADER = """#version 300 es
precision highp float;
uniform sampler2D uTex;
uniform vec2 uCenter;
uniform float uRadius;
uniform vec2 uRotation; // cos, sin of the clockwise image rotation
uniform float uZoom;
uniform vec2 uPan;      // circle centre offset from uCenter, pixels, y down
out vec4 color;
void main() {
    // Offset from the circle centre in unzoomed pixels, y pointing down like the bitmap's rows.
    vec2 p = (vec2(gl_FragCoord.x - uCenter.x, uCenter.y - gl_FragCoord.y) - uPan) / uZoom;
    float inside = clamp((uRadius - length(p)) * uZoom + 0.5, 0.0, 1.0);
    // Undo the image rotation to find where this pixel samples it.
    vec2 q = vec2(uRotation.x * p.x + uRotation.y * p.y, uRotation.x * p.y - uRotation.y * p.x);
    vec2 uv = q / (2.0 * uRadius) + 0.5;
    color = vec4(texture(uTex, uv).rgb * inside, 1.0);
}
"""
    }
}
