# Contributing

Thanks for helping improve Vibe Voice Lite.

## Project Stage

The project is in alpha. Small, focused pull requests are easiest to review. Please avoid broad rewrites unless an issue or discussion has already agreed on the direction.

## Development Setup

Requirements:

- Windows
- Rust stable toolchain with edition 2024 support
- PowerShell
- A microphone
- Optional: a local SenseVoice-compatible model directory for transcription testing

Run the app:

```powershell
cargo run --bin vibe-voice-lite
```

Run checks:

```powershell
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
```

Use a Visual Studio Developer PowerShell with Windows SDK `rc.exe` on PATH.
Run `cargo test --locked --all-targets` for the normal regression suite. Real
model/device tests are ignored by default. To opt in, set `VIBE_TEST_MODEL` to
the official SenseVoice directory (including `test_wavs/zh.wav`) and
`VIBE_TTS_RUNTIME_DIR` to a configured `tts` directory, then run named ignored
tests with `-- --ignored --test-threads=1`. GPU/speech tests play synthetic audio.

Live AI tests require the separate explicit `VIBE_TEST_LIVE_AI=1` flag and a
`VIBE_CONFIG_DIR` containing your locally configured settings. They send
synthetic test phrases to your chosen provider and may incur API usage.
Never run them with credentials stored in repository files or CI secrets.

Before publication, run `python scripts/audit_release.py` against the staged
Git index. It is a heuristic check, not a comprehensive security audit. Keep
personal settings, TTS runtime paths, model weights and recordings out of Git.

## Pull Request Checklist

Before opening a pull request:

- Keep the change scoped to one topic.
- Run formatting and checks.
- Update `README.md` when setup, behavior, or user-facing workflows change.
- Do not commit model files, recordings, generated binaries, local paths, or secrets.
- Mention the Windows version and model directory shape you tested when relevant.

## Issue Guidelines

For bugs, include:

- Windows version
- App version or commit SHA
- Steps to reproduce
- Expected behavior
- Actual behavior
- Model directory shape, without uploading proprietary model files
- Screenshots or short screen recordings when UI behavior matters

For feature requests, explain the workflow you want to improve and why the current behavior is not enough.

## Code Style

- Follow existing Rust and egui patterns in the repository.
- Prefer small functions over large UI rewrites.
- Keep platform-specific behavior explicit.
- Avoid adding network behavior unless it is clearly documented and optional.
