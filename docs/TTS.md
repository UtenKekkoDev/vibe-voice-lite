# Local Qwen3-TTS setup

TTS is optional. Dictation and AI text editing work without it. The local streaming
runtime is tested on Windows 11, Python 3.12, an NVIDIA RTX 5080 and CUDA PyTorch.
It is not a CPU-only implementation. Allow roughly 5 GB of free VRAM, with more
headroom for other GPU workloads. On the test RTX 5080, a question queued at
worker startup reached the first non-silent audio callback in 8.50–8.66 seconds
across three fresh worker processes. The same question on a ready worker took
0.29–0.31 seconds. These measurements include local model startup when applicable.
OS file caches, driver state and other workloads affect the result.

1. Install Python 3.12 and a compatible NVIDIA driver.
2. From the app/repository directory, create a virtual environment:

   ```powershell
   py -3.12 -m venv .venv-tts
   .\.venv-tts\Scripts\python.exe -m pip install torch==2.10.0 --index-url https://download.pytorch.org/whl/cu128
   .\.venv-tts\Scripts\python.exe -m pip install -r tts/requirements.txt
   ```

3. Download `Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice` from the
   [official Qwen model repository](https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice),
   including `speech_tokenizer/`. The tested revision is
   `0c0e3051f131929182e2c023b9537f8b1c68adfe`.

   ```powershell
   .\.venv-tts\Scripts\python.exe -c "from huggingface_hub import snapshot_download; snapshot_download('Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice', revision='0c0e3051f131929182e2c023b9537f8b1c68adfe', local_dir='models/Qwen3-TTS-12Hz-1.7B-CustomVoice')"
   ```

4. Copy `tts/runtime.example.json` to `tts/runtime.json`. Paths are relative to
   the `tts` directory; absolute local paths also work. Set `python` and `model`
   to your installations. Do not commit this machine-specific file.
5. Start the app and use Settings → restart voice / test question.

The model and Python environment are not bundled in source or release ZIPs.
Downloading/installing uses the network; subsequent TTS inference is offline.
The default voice is Serena, with the selected gentle, slightly slower style.
Each utterance uses the same seed and instruction. Different sentences can still
have different natural emphasis and pauses. This is not a voice-cloning mode.

Serena reads AI questions, mode selection prompts, voice selection confirmations and a short receipt after a successful interview summary. Normal
feedback, final summaries and errors remain text-only. Hold the talk key to
interrupt speech. The child process is owned by a Windows Job Object and ends
when the app exits, including during model loading.

Before each question or preview, Serena checks the current Windows default
playback device. If you switch to headphones between questions, the next one
uses that device and the loaded model stays in memory.

## Upgrading to a new folder

Exit the old app, extract the new ZIP and copy your old `tts/runtime.json` to the
new `tts` directory. Keep the new worker scripts. Relative paths in the copied
configuration now start at the new `tts` directory. If your Python environment
and model remain in the old app folder, edit both path fields to use their actual
absolute locations. For example, for an old installation in `D:/Apps/VoiceOld`:

```json
{
  "python": "D:/Apps/VoiceOld/.venv-tts/Scripts/python.exe",
  "model": "D:/Apps/VoiceOld/models/Qwen3-TTS-12Hz-1.7B-CustomVoice"
}
```

Preserve your existing `style_instruction` field when editing the two paths.
With the old environment still present, start the new app and use Settings → test
question to confirm playback.
