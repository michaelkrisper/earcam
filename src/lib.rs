//! Desktop client for LMECHN and other i4season WiFi ear cameras.
//!
//! Everything but the window lives in this library so it can be tested without a display
//! or a camera: [`protocol`] and [`assembler`] turn UDP datagrams into JPEG frames,
//! [`stream`] talks to the camera and decodes, [`orientation`] turns the probe's gravity
//! vector into a screen rotation, [`wifi`] switches the WiFi to the camera and back, and
//! [`render`] draws with OpenGL.

pub mod assembler;
pub mod orientation;
pub mod protocol;
pub mod render;
pub mod signals;
pub mod stream;
pub mod wifi;
