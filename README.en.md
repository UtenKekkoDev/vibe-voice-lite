# Vibe Voice Lite

<img src="assets/app-icon.png" width="112" alt="Vibe Voice Lite">

A Windows hold-to-talk app with local speech recognition, optional AI editing, requirement interviews and local streaming TTS. [中文说明](README.md)

## Use

Click the destination text field, hold **Right Ctrl**, speak, then release. The tray menu provides settings, three mutually exclusive modes, automatic paste and quit.

- **Dictation**: transcribe directly; optional AI cleanup.
- **Code editing**: organize spoken development requirements without inventing missing details.
- **Requirement interview**: one question at a time, spoken through Serena when TTS is enabled. Hold Right Ctrl to answer. Undecided items may be skipped.

In interview mode, say **“先整理到这里”** as a separate utterance to finish and obtain **已确定** (confirmed) and **待定** (pending) sections. Say **“取消这次整理”** or **“先不处理”** to cancel a pending conversation. Escape cancels while waiting for an answer; switching modes also cancels. “停止” alone is not a fixed command.

Only clarification/interview questions are spoken automatically. Ordinary feedback, final summaries, errors and completion receipts remain text-only. Holding the talk key interrupts TTS. Conversations are held in process memory and are not restored after exit.

Final text is copied and Ctrl+V is sent only if the selected external window is still foreground and no conflicting modifiers are held. The app cannot verify that every third-party editor accepted the shortcut. Copy/paste failures preserve the text; manual Ctrl+V is available after successful clipboard copy.

## Setup

Open the latest successful main run in [Windows builds](https://github.com/UtenKekkoDev/vibe-voice-lite/actions/workflows/ci.yml?query=branch%3Amain). Sign in to GitHub and download **windows-ci-bundle** under **Artifacts**. Extract it, then unzip `vibe-voice-lite-windows-x64.zip` and launch `vibe-voice-lite.exe`. Published versions appear under [Releases](https://github.com/UtenKekkoDev/vibe-voice-lite/releases).

Windows 10/11 x64, a microphone and the [Microsoft Visual C++ x64 Redistributable](https://aka.ms/vs/17/release/vc_redist.x64.exe) are required. Release ZIPs are unsigned and exclude models, credentials, Python and CUDA.

- Local recognition: install the [SenseVoice model](docs/MODELS.md), select its directory and run the check in Settings.
- Optional cloud recognition: configure Qwen-ASR Realtime; audio is transmitted to that provider.
- AI editing/interviews: configure DeepSeek, OpenAI, Ollama or a compatible Chat Completions endpoint, an available model and credentials.
- Optional TTS: [setup guide](docs/TTS.md). Qwen3-TTS 1.7B CustomVoice / Serena uses a gentle, slightly slower fixed style. An NVIDIA CUDA GPU is required by the current worker. Its Python runtime and weights are installed separately.

## Privacy

Settings are saved under `%LOCALAPPDATA%/VibeVoiceLite/settings.json`. The AI key is encrypted with Windows DPAPI for the current user and bound to its endpoint. Changing endpoints/providers clears the old key. Qwen-ASR keys are session-only.

Recordings, transcripts and interview history are not written to settings. Lifecycle logs contain state/performance metadata; opt-in debug logs may contain text and should not be shared. **AI editing and interviews send transcripts, glossary and prior answers to the configured service.** Local ASR/TTS inference itself does not upload content. See [SECURITY.md](SECURITY.md).

`tts/runtime.json` contains local paths and is ignored by Git. Copy `runtime.example.json` and edit your local copy. Seed/style consistency does not force identical intonation across different sentences.

## Build

Install Rust stable, Visual Studio C++ Build Tools and Windows SDK. Use a **Developer PowerShell for VS** with `rc.exe` on PATH (or set `RC`):

```powershell
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release --bin vibe-voice-lite
python -m compileall -q tts scripts
python scripts/audit_release.py
./scripts/package.ps1
```

The first build downloads Rust dependencies and ONNX Runtime. Official pollster 0.4.0 is vendored without machine-specific paths. `run-native.ps1` sets the source TTS directory for development.

`package.ps1` creates a ZIP and SHA-256 file containing runtime code, required adjacent DLLs, documentation and a TTS config example. It excludes models and personal configuration. Integration tests for real models, GPU/audio and live AI are opt-in; see [CONTRIBUTING.md](CONTRIBUTING.md).

Original icon artwork is in `assets/app-icon.svg`; the dependency-free `scripts/generate_icons.py` generates PNG, multi-resolution ICO and runtime RGBA. The executable, app window and tray share this artwork.

Based on [vibe-coding-voice](https://github.com/UtenKekkoDev/vibe-coding-voice). MIT; see [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
