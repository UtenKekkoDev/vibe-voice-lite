# Third-Party Notices

## Local Qwen TTS runtime

- Qwen3-TTS 1.7B CustomVoice and its tokenizer: Alibaba Qwen, Apache-2.0. Model revision `0c0e3051f131929182e2c023b9537f8b1c68adfe`.
- faster-qwen3-tts v0.3.2, revision `a70afc0f81f7f5f8801c3227968f1102f43f211c`: MIT; license included in `tts/Faster-Qwen-LICENSE`.
- The separately provisioned Python runtime uses PyTorch, transformers, NumPy, sounddevice/PortAudio and qwen-tts. Their original distributions and license files remain in that runtime. No model weights are embedded in the executable.

This file summarizes third-party components and model-related notice boundaries for Vibe Voice Lite.

## Source Code License

The source code in this repository is licensed under the MIT License. See [LICENSE](LICENSE).

## Rust Dependencies

This project depends on open-source Rust crates listed in `Cargo.toml` and locked in `Cargo.lock`, including:

- `anyhow` for error handling
- `eframe` / `egui` for the native UI
- `cpal` for microphone capture
- `transcribe-rs` for the SenseVoice transcription path
- `ort` and ONNX Runtime binaries for local ONNX model execution
- `tungstenite`, `native-tls`, and `base64` for the Qwen-ASR Realtime WebSocket path
- `GetAsyncKeyState` polling for hold-to-talk (Windows API)
- `tray-icon` for Windows tray integration
- `arboard` for clipboard and Windows `SendInput` for keyboard delivery
- `serde_json` for JSON parsing and serialization
- `windows-sys` for Windows API calls

Each dependency is governed by its own license. Check the crate metadata from crates.io or run a license audit tool such as `cargo-deny` before redistributing binaries in a stricter compliance environment.

## Local PowerShell Helper

`recording-overlay.ps1` is a historical project script and is not used or bundled by this build. The app uses a small PowerShell/Windows Forms folder picker; recording status is rendered in the native window.

## App Assets And Documentation Media

Files under `assets/` and `docs/assets/` are project-owned UI/documentation assets unless a file explicitly states otherwise. They are included only to support the app UI and README documentation.

## Speech Recognition Models

Model weights are not included in this repository and are not covered by this repository's MIT License.

The recommended SenseVoice-compatible model is downloaded separately from the sherpa-onnx model releases:

```text
sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17
```

Reference: [sherpa-onnx SenseVoice pre-trained models](https://k2-fsa.github.io/sherpa/onnx/sense-voice/pretrained.html).

Before using, redistributing, or bundling any model, review that model's license and terms. In particular, confirm:

- the contents of the downloaded model's `LICENSE` and `README.md`, if present
- whether commercial use is permitted
- whether redistribution is permitted
- whether attribution is required
- whether converted or quantized artifacts can be shared

## Online Speech Recognition Services

When users choose the online Qwen-ASR Realtime path, audio is sent to the configured DashScope WebSocket endpoint. Users are responsible for their own API key, account terms, service region, data handling requirements, and usage costs.

## Not Bundled

The repository intentionally excludes:

- ONNX model files
- local model directories
- microphone recordings
- generated audio files
- environment files and logs

See `.gitignore` for the current exclusion list.

## Vendored pollster

Official pollster 0.4.0 (MIT OR Apache-2.0), with both license files in vendor/pollster. Original crate archive SHA-256: 2f3a9f18d041e6d0e102a0a46750538147e5e8992d3b4873aaafee2520b00ce3. This replaces the former local compatibility stub.
