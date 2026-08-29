package com.krisper.earcam.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.krisper.earcam.CameraUiState
import com.krisper.earcam.Ear
import com.krisper.earcam.R
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin

@Composable
fun CameraScreen(
    state: CameraUiState,
    onSelectEar: (Ear) -> Unit,
    onToggleDebug: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
            .pointerInput(Unit) { detectTapGestures(onLongPress = { onToggleDebug() }) },
        contentAlignment = Alignment.Center,
    ) {
        val image = state.image
        if (image != null) {
            Canvas(Modifier.fillMaxSize()) { drawUpright(image, state.angle) }
        } else {
            BasicText(
                text = if (state.error) stringResource(R.string.no_camera) else state.status,
                style = TextStyle(color = Color.White, fontSize = 16.sp),
            )
        }

        if (state.debug && state.debugText.isNotEmpty()) {
            BasicText(
                text = state.debugText,
                style = TextStyle(color = Color.White, fontSize = 12.sp),
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(16.dp),
            )
        }

        EarSelector(
            selected = state.ear,
            onSelect = onSelectEar,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .navigationBarsPadding()
                .padding(bottom = 24.dp),
        )
    }
}

/**
 * Two-segment pill. Hand-rolled rather than pulled from Material 3, which would add
 * megabytes to the APK for the only piece of chrome this app has.
 */
@Composable
private fun EarSelector(selected: Ear, onSelect: (Ear) -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(percent = 50))
            .background(Color.White.copy(alpha = 0.12f))
    ) {
        EarSegment(R.string.ear_left, selected == Ear.LEFT) { onSelect(Ear.LEFT) }
        EarSegment(R.string.ear_right, selected == Ear.RIGHT) { onSelect(Ear.RIGHT) }
    }
}

@Composable
private fun EarSegment(labelRes: Int, active: Boolean, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .clip(RoundedCornerShape(percent = 50))
            .background(if (active) Color.White.copy(alpha = 0.22f) else Color.Transparent)
            .clickable(onClick = onClick)
            .padding(horizontal = 24.dp, vertical = 12.dp),
        contentAlignment = Alignment.Center,
    ) {
        BasicText(
            text = stringResource(labelRes),
            style = TextStyle(
                color = if (active) Color.White else Color.White.copy(alpha = 0.6f),
                fontSize = 15.sp,
                fontWeight = if (active) FontWeight.Medium else FontWeight.Normal,
            ),
        )
    }
}

/**
 * Draws the full sensor image centred and counter-rotated, scaled so no corner is ever
 * clipped. Nothing is cropped or masked into a circle — the camera's optics already
 * paint a dark vignette into the corners, and that is left visible.
 */
private fun DrawScope.drawUpright(image: ImageBitmap, angle: Float) {
    val w = image.width.toFloat()
    val h = image.height.toFloat()

    // A rotated w×h rectangle occupies w|cos| + h|sin| by w|sin| + h|cos|.
    val rad = Math.toRadians(angle.toDouble())
    val c = abs(cos(rad)).toFloat()
    val s = abs(sin(rad)).toFloat()
    val scale = min(size.width / (w * c + h * s), size.height / (w * s + h * c))

    val dstW = (w * scale).roundToInt()
    val dstH = (h * scale).roundToInt()

    withTransform({ rotate(angle, center) }) {
        drawImage(
            image = image,
            srcOffset = IntOffset.Zero,
            srcSize = IntSize(image.width, image.height),
            dstOffset = IntOffset(
                (center.x - dstW / 2f).roundToInt(),
                (center.y - dstH / 2f).roundToInt(),
            ),
            dstSize = IntSize(dstW, dstH),
            filterQuality = FilterQuality.High,
        )
    }
}
