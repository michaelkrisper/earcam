# Contributing to EarCam

Thanks for your interest. Bug reports, compatibility reports for other cameras and pull
requests are all welcome.

## Workflow

1. Every change starts with an issue: a bug report or a feature request.
2. Work on a branch named after the issue. `gh issue develop <issue> --checkout`
   creates `<issue>-<title>`.
3. Open a pull request against `main`. The _Tickets_ check links the issue
   (`Closes #<issue>`); a pull request without a ticket fails.
4. CI runs on every pull request. When all checks are green, _Auto-merge_
   squash-merges it. Open the pull request as a draft while it is not ready.
5. The merge closes the issue. `main` always holds the latest valid state
   (trunk-based development).

## Versions and releases

- Versions follow [Semantic Versioning](https://semver.org) and exist only as
  git tags (`vMAJOR.MINOR.PATCH`).
- CI tags every merge to `main` with the next patch version.
- A release bundles the changes since the previous one: run
  _Actions → Release → Run workflow_ and pick `minor` or `major`. It tags
  `main` and publishes a GitHub release listing the merged pull requests and
  closed tickets.

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
Android on every pull request. Keep commits focused and describe *why* in the commit
message and the pull request; the release notes are built from the pull request titles.

Changes to the wire format should come with a test, ideally in `tests/fake_camera.rs`,
and an update to [docs/protocol.md](docs/protocol.md).

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
