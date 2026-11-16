"""Render cross-sentence style samples through the production streaming path."""
import json
import pathlib
import sys
import time

root = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(root))
from qwen_stream import QwenStream
import numpy as np
import soundfile as sf

output = pathlib.Path(sys.argv[1])
output.mkdir(parents=True, exist_ok=True)
config = json.loads((root / "runtime.json").read_text(encoding="utf-8-sig"))
engine = QwenStream(config["model"], instruct=None)
engine.warmup()
questions = [
    "这个待办工具主要给你自己用，还是也要和别人共享？",
    "你最希望先解决的，是快速记录事情，还是按时提醒？",
    "遇到暂时没有决定的事情，你希望放进待定列表吗？",
]
styles = {
    "consistent": "保持自然的说话方式，语速适中且平稳，音量均匀，语气起伏小。",
    "neutral": "用平稳中性的日常助手语气说话。语速适中，发音清楚，音量均匀，情绪克制，句末轻微上扬，避免夸张重音和播音腔。",
    "gentle": "用轻柔亲切的日常交谈语气说话。语速稍慢，音量柔和且稳定，停顿自然，句末轻轻上扬，不撒娇，不夸张，不使用播音腔。",
}
results = []
for name, instruction in styles.items():
    engine.instruct = instruction
    # Prefill shape is different with an instruction; warm it before timing.
    list(engine.stream(questions[0]))
    combined = []
    for index, text in enumerate(questions):
        start = time.perf_counter()
        chunks = []
        first_ms = None
        for audio, sr, _ in engine.stream(text):
            if first_ms is None:
                first_ms = (time.perf_counter() - start) * 1000
            chunks.append(audio)
        pcm = np.concatenate(chunks)
        sf.write(output / f"{name}-{index+1}.wav", pcm, sr)
        combined.extend([pcm, np.zeros(int(sr * .7), dtype=np.float32)])
        results.append({"style": name, "text": text, "first_pcm_ms": round(first_ms, 1), "audio_seconds": round(len(pcm)/sr, 2)})
    sf.write(output / f"{name}-three-questions.wav", np.concatenate(combined), sr)
(output / "comparison.json").write_text(json.dumps({"instructions": styles, "results": results}, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(results, ensure_ascii=False), flush=True)
