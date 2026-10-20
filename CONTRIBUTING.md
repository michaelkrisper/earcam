# Contributing to EarCam

Thanks for your interest. Bug reports, compatibility reports for other cameras and pull
requests are all welcome.

## Reporting a camera

Whether EarCam works with your camera or not, please
[open a camera report](https://github.com/michaelkrisper/earcam/issues/new?template=camera_report.yml)
with the brand and model, the platform you used, and the `camera ...` line from the
log (terminal on the desktop, `adb logcat -s EarCam` on Android).

## Reporting a bug

[Open a bug report](https://github.com/michaelkrisper/earcam/issues/new?template=bug_report.yml)
with the steps to reproduce, what you expected, and the log.

## Pull requests

EarCam is deliberately minimal: one window with the live image, no menus and no
settings. Please open an issue to discuss a new feature before building it.

Before you submit, make sure these pass:

```
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

If you touch platform code, also run clippy for the other targets (see
[Building from source](README.md#building-from-source)); CI checks Linux, Windows and
Android on every pull request. Keep commits focused, describe *why* in the commit
message, and add an entry under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for
user-visible changes.

Changes to the wire format should come with a test, ideally in `tests/fake_camera.rs`,
and an update to [docs/protocol.md](docs/protocol.md).

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
