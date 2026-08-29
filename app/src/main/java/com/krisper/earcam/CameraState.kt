package com.krisper.earcam

import androidx.compose.ui.graphics.ImageBitmap

enum class Ear { LEFT, RIGHT }

data class CameraUiState(
    val image: ImageBitmap? = null,
    /** Rotation applied so the image stays upright; the ear offset is already included. */
    val angle: Float = 0f,
    val ear: Ear = Ear.RIGHT,
    val error: Boolean = false,
    val debug: Boolean = false,
    val debugText: String = "",
)
