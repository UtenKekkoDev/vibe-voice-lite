"""Real-GPU regression: repeatability after other text, cancellation and restart.

Run twice with an output directory, adding --verify-restart on the second run.
Does not play audio or use personal speech.
"""
import argparse
import hashlib
import json
import pathlib
import sys
import time

import numpy as np
import soundfile as sf

root = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(root))
from qwen_stream import QwenStream
parser = argparse.ArgumentParser()
parser.add_argument("output", type=pathlib.Path)
parser.add_argument("--verify-restart", action="store_true")
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
config = json.loads((root / "runtime.json").read_text(encoding="utf-8-sig"))
engine = QwenStream(config["model"], instruct=config.get("style_instruction") or None)
engine.warmup()
question = "需要确认一下：你说的是访问令牌，还是刷新令牌？"


def render(text):
    start = time.perf_counter()
    chunks = []
    first_ms = None
    for audio, rate, info in engine.stream(text):
        if first_ms is None:
            first_ms = (time.perf_counter() - start) * 1000
        chunks.append(audio)
    pcm = np.concatenate(chunks)
    return pcm, engine.last_codes.cpu().numpy().copy(), {
        "first_pcm_ms": round(first_ms, 1),
        "generation_ms": round((time.perf_counter() - start) * 1000, 1),
        "audio_seconds": round(len(pcm) / rate, 3),
        "pcm_sha256": hashlib.sha256(pcm.tobytes()).hexdigest(),
    }


def check_decode_masks():
    """The optimized table must match single-token HF masks at padding boundaries."""
    import torch
    from transformers.masking_utils import create_causal_mask

    graph = engine.model.talker_graph
    dummy = torch.zeros(1, 1, graph.hidden_size, dtype=graph.dtype, device=graph.device)
    with torch.inference_mode():
        for padding in (None, 0, 3):
            mask = None
            if padding is not None:
                mask = torch.ones(1, graph.max_seq_len, dtype=torch.long, device=graph.device)
                mask[:, :padding] = 0
            graph._build_attention_masks(mask)
            for position in (0, 2, 3, graph.max_seq_len // 2, graph.max_seq_len - 1):
                expected = create_causal_mask(
                    config=graph.model.config,
                    input_embeds=dummy,
                    attention_mask=mask,
                    cache_position=torch.tensor([position], device=graph.device),
                    past_key_values=graph.static_cache,
                )
                assert torch.equal(graph.attn_mask_table[position], expected), (padding, position)


first, first_codes, timing = render(question)
baseline = args.output / "baseline.npz"
if args.verify_restart:
    saved = np.load(baseline)
    assert np.array_equal(saved["codes"], first_codes), "Restart changed codec tokens"
    assert np.array_equal(saved["pcm"], first), "Restart changed PCM"
    print("PASS restart", json.dumps(timing), flush=True)
else:
    np.savez(baseline, pcm=first, codes=first_codes)
    sf.write(args.output / "serena-stable-question.wav", first, 24000)
    render("你希望修改登录页，还是注册页？")
    interrupted = engine.stream("这是一段会在第一段音频生成后取消的测试内容。")
    next(interrupted)
    interrupted.close()
    repeated, repeated_codes, repeated_timing = render(question)
    assert np.array_equal(first_codes, repeated_codes), "Prior requests changed codec tokens"
    assert np.array_equal(first, repeated), "Prior requests changed PCM"
    report = {"initial": timing, "after_other_text_and_cancel": repeated_timing}
    (args.output / "stability.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print("PASS repeat after other text and cancellation", json.dumps(report), flush=True)

check_decode_masks()
