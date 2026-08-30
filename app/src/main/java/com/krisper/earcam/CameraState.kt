package com.krisper.earcam

import androidx.compose.ui.graphics.ImageBitmap

data class CameraUiState(
    val image: ImageBitmap? = null,
    /** Rotation applied so the image stays upright. */
    val angle: Float = 0f,
    /** What the app is currently doing, shown until the first frame arrives. */
    val status: String = "",
    val error: Boolean = false,
    val debug: Boolean = false,
    val debugText: String = "",
)
