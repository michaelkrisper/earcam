package com.krisper.earcam.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.krisper.earcam.CameraUiState
import com.krisper.earcam.R
import kotlin.math.roundToInt

@Composable
fun CameraScreen(
    state: CameraUiState,
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

    }
}


/**
 * Draws the sensor image centred and counter-rotated inside a circular mask.
 *
 * The mask hides the corners of the square image, which would otherwise sweep around
 * and betray every bit of rotation. It is deliberately a little wider than the screen:
 * the sides of the circle leave the viewport, so the visible edge reads as a frame
 * rather than a porthole. The image is sized so its inscribed circle covers the mask
 * at any angle.
 */
private fun DrawScope.drawUpright(image: ImageBitmap, angle: Float) {
    val radius = size.minDimension / 2f * MASK_OVERSCAN
    val side = (2f * radius).roundToInt()

    val mask = Path().apply { addOval(Rect(center, radius)) }
    clipPath(mask) {
        withTransform({ rotate(angle, center) }) {
            drawImage(
                image = image,
                srcOffset = IntOffset.Zero,
                srcSize = IntSize(image.width, image.height),
                dstOffset = IntOffset(
                    (center.x - side / 2f).roundToInt(),
                    (center.y - side / 2f).roundToInt(),
                ),
                dstSize = IntSize(side, side),
                filterQuality = FilterQuality.High,
            )
        }
    }
}

private const val MASK_OVERSCAN = 1.1f
