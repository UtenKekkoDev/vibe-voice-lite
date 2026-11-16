# Local Qwen3-TTS setup

TTS is optional. Dictation and AI text editing work without it. The local streaming
runtime is tested on Windows 11, Python 3.12, an NVIDIA RTX 5080 and CUDA PyTorch.
It is not a CPU-only implementation. Allow roughly 5 GB of free VRAM, with more
headroom for other GPU workloads. First load and warmup on the tested PC takes
about 9–12 seconds; hardware and drivers affect timings.

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

Only AI clarification and interview questions are spoken automatically. Normal
feedback, final summaries and errors remain text-only. Hold the talk key to
interrupt speech. The child process is owned by a Windows Job Object and ends
when the app exits, including during model loading.
