<p align="center"><img src="assets/app-icon.png" width="80" alt=""></p>
<h1 align="center">Vibe Voice Lite</h1>
<p align="center">Hold Right Ctrl in a Windows text field to dictate. Turn a spoken idea into a development brief, or work through it one question at a time.</p>
<p align="center"><strong><a href="https://github.com/UtenKekkoDev/vibe-voice-lite/releases/download/v0.2.0-preview.3/vibe-voice-lite-windows-x64.zip">Download for Windows x64 · preview.3</a></strong> &nbsp;·&nbsp; <a href="https://github.com/UtenKekkoDev/vibe-voice-lite/releases/tag/v0.2.0-preview.3">Release notes</a> &nbsp;·&nbsp; <a href="README.md">中文</a></p>

<p align="center"><img src="assets/result-card.png" width="520" alt="Floating result card showing a completed Chinese transcription"></p>

## First run

1. Extract the entire ZIP and launch `vibe-voice-lite.exe`. The unsigned preview supports Windows 10/11 x64 and requires a microphone and the [Microsoft Visual C++ x64 Redistributable](https://aka.ms/vs/17/release/vc_redist.x64.exe).
2. Choose a recognition provider in Settings. Install a local [SenseVoice INT8 model](docs/MODELS.md) or configure Qwen-ASR Realtime. Run the device and recognition check.
3. Click the destination field, hold **Right Ctrl**, speak, then release. The tray menu lets you change modes, configure the talk key, toggle automatic paste, or copy the last completed result.

## Choose a mode

| Mode | When to use it | What you get |
| --- | --- | --- |
| 1 · Voice dictation (语音听写) | Messages and notes | A transcript with optional AI cleanup |
| 2 · Developer instructions (开发指令) | Describing changes to a coding assistant | A brief with goals, constraints and unresolved details |
| 3 · Requirements Q&A (需求问答) | Exploring an idea before implementation | One question per turn, followed by confirmed and pending items when you finish |

**Switch by voice.** Hold Right Ctrl and say “第二” or “切换第二个” to go straight to developer instructions. To hear the choices first, say “Switch mode”, “Switch type” or “切换模式”, then hold the key again and answer with a number or mode name. Serena confirms the selection when enabled. Say “cancel” or press Esc to return to an existing interview. To select an option within an interview question, say “我选第二个” (I choose the second option).

### Interview commands

Hold the talk key to interrupt speech and answer. Speak each command as a separate utterance.

| Command | Action |
| --- | --- |
| **直接输入** or **直出** | Deliver only your last transcribed utterance verbatim and end the current interview. Append “，这句直接输入” to deliver just the preceding text from the same recording. |
| **这个先待定** | Park the current question. You can return to it later. |
| **先整理到这里** | Finish and summarize. A suggestion to wrap up still leaves room for more input. |
| **取消这次整理** or Esc while waiting | Cancel the conversation. Switching modes or exiting also ends it. |

During an interview, “OK，就这样” or “停止” also wraps up. With speech enabled, Serena acknowledges the completed summary.

## AI and spoken questions

Local dictation works on its own. For AI editing or interviews, configure DeepSeek, OpenAI, Ollama or a compatible Chat Completions endpoint with a model name and credentials as needed. Remote AI endpoints require HTTPS. Loopback servers can use HTTP.

Optional **Qwen3-TTS 1.7B CustomVoice / Serena** reads clarification questions, interview questions, mode prompts and confirmations for mode changes and completed interviews aloud. This backend requires an NVIDIA CUDA GPU. Install model weights, Python and CUDA separately. The model stays loaded between questions and is released on speech restart or app exit. [TTS setup](docs/TTS.md)

### Serena startup measurements (preview.1 and preview.2)

Both versions used the same question on an RTX 5080. We alternated versions and started a fresh worker three times per version. Times are medians from submitting the request to the first non-silent audio in the device callback.

| Scenario | preview.1 | preview.2 |
| --- | ---: | ---: |
| Newly started worker | 10.36 s | **8.64 s** |
| Ready worker | 307 ms | **298 ms** |

All 107,520 audio samples and all six streaming chunk boundaries matched for the test sentence. Settings also shows cumulative recognition, AI and playback timings for the last eight turns, starting when the talk key is released.

## Privacy and paste

- Local recognition and speech synthesis run on-device. Cloud recognition sends audio to your provider. AI editing and interviews send transcripts, glossary entries and conversation history to the selected endpoint.
- Settings live at `%LOCALAPPDATA%/VibeVoiceLite/settings.json`. Windows DPAPI protects the AI key for the current user and endpoint. Qwen-ASR credentials last for the session. Recordings and transcripts stay out of settings.
- Automatic paste checks the foreground window, focused control and window title. Some pages reuse all three, and elevated apps may block synthetic input. Use **复制上次结果** in the tray to recover the last completed text. Its in-memory copy clears on exit. Windows manages the clipboard.
- AI failure cards show the reason. The local lifecycle log records the mode, whether a complete response arrived and the diagnostic reason. Opt-in debug logs may contain text. Local TTS paths are stored in `tts/runtime.json`. Review attachments before sharing a report. [Security policy](SECURITY.md)

## Build from source

Install Rust stable, Visual Studio C++ Build Tools and the Windows SDK. From a **Developer PowerShell for VS** with `rc.exe` on PATH, run:

```powershell
cargo build --locked --release --bin vibe-voice-lite
./scripts/package.ps1
```

The first build downloads dependencies and ONNX Runtime. Packaging generates the Windows ZIP and its SHA-256 checksum. [Tests, device checks and release workflow](CONTRIBUTING.md)

Based on [vibe-coding-voice](https://github.com/UtenKekkoDev/vibe-coding-voice). [MIT license](LICENSE) · [Third-party notices](THIRD_PARTY_NOTICES.md)
