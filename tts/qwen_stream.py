"""Local Qwen3-TTS BF16 streaming runner, pinned CUDA graph implementation.

Uses a local CustomVoice model and Serena voice. Keeps the complete
codec prefix while decoding short replies, avoiding context truncation between
chunks. A single persistent PortAudio stream prevents per-chunk device restarts.
"""
from __future__ import annotations
import os, sys, pathlib, time, queue, threading
os.environ['HF_HUB_OFFLINE']='1'
os.environ['TRANSFORMERS_OFFLINE']='1'
os.environ['TOKENIZERS_PARALLELISM']='false'
ROOT=pathlib.Path(__file__).resolve().parent
SOURCE=ROOT
sys.path.insert(0,str(SOURCE))
import numpy as np
import torch
from faster_qwen3_tts import FasterQwen3TTS
from faster_qwen3_tts.streaming import fast_generate_streaming

SENTENCES=[
    ('ack','已整理完成。'),
    ('question','需要确认一下：你说的是访问令牌过期，还是刷新令牌过期？'),
    ('mixed','你说的是 access token 过期，还是 refresh token 过期？请按住右 Ctrl 补充。'),
    ('requirement','登录失败的提示改成中文。令牌过期后先尝试刷新，只有刷新失败才返回登录页。请保留原来的布局，并补充刷新失败的测试。'),
]

class QwenStream:
    def __init__(self,model_dir=None,instruct=None):
        torch.set_num_threads(4)
        self.instruct=instruct
        self.model=FasterQwen3TTS.from_pretrained(str(model_dir or ROOT/'qwen-0.6b-customvoice'),
            device='cuda:0',dtype=torch.bfloat16,attn_implementation='sdpa',max_seq_len=1024)
        if instruct and self.model.model.model.tts_model_size=='0b6':
            raise ValueError('0.6B CustomVoice does not support style instructions')
        self.sample_rate=self.model.speech_tokenizer.get_output_sample_rate()
        self.last_codes=None
        self.generation_seed=42

    def warmup(self):
        self.model.warmup()
        # One streamed chunk exercises prefill, decode and the vocoder.
        # Each real utterance resets the seed and KV cache before generation.
        probe = self.stream('已整理完成。')
        try:
            next(probe)
        finally:
            probe.close()

    @torch.inference_mode()
    def decode(self,codes):
        audio,sr=self.model.speech_tokenizer.decode({'audio_codes':codes.unsqueeze(0)})
        a=audio[0]
        if hasattr(a,'cpu'):
            a=a.flatten().cpu().numpy()
        return np.asarray(a,dtype=np.float32).flatten(),sr

    @torch.inference_mode()
    def stream(self,text,chunk_size=6,steady_chunk_size=12,context_frames=None):
        if not 1 <= chunk_size <= 25:
            raise ValueError('chunk_size must be between 1 and 25 codec frames')
        if steady_chunk_size < chunk_size or steady_chunk_size % chunk_size:
            raise ValueError('steady_chunk_size must be a positive multiple of chunk_size')
        if not text.strip():
            raise ValueError('Text must not be empty')
        # Reset both CPU and CUDA RNG at each utterance, including after a
        # cancelled stream. Keep Serena's default sampling/style parameters;
        # previous requests must not randomly change the next rendition.
        torch.manual_seed(self.generation_seed)
        self.last_codes=None
        m,talker,config,tie,tam,tth,tpe=self.model._prepare_generation_custom(
            text=text,language='Chinese',speaker='Serena',instruct=self.instruct,non_streaming_mode=True)
        # Reject overlong requests rather than silently stopping at static-cache limit.
        max_tokens=min(512,self.model.max_seq_len-tie.shape[1]-2)
        if max_tokens < 64:
            raise ValueError('Text exceeds the short-reply streaming context budget')
        codes=[]
        emitted=0
        pending_frames=0
        last_info=None
        for block,timing in fast_generate_streaming(talker=talker,talker_input_embeds=tie,
                attention_mask=tam,trailing_text_hiddens=tth,tts_pad_embed=tpe,config=config,
                predictor_graph=self.model.predictor_graph,talker_graph=self.model.talker_graph,
                max_new_tokens=max_tokens,chunk_size=chunk_size,
                temperature=.9,top_k=50,top_p=1.,do_sample=True,repetition_penalty=1.05):
            codes.append(block)
            pending_frames += len(block)
            last_info=timing
            if emitted and pending_frames < steady_chunk_size and not timing['is_final']:
                continue
            prefix=torch.cat(codes,dim=0)
            start_frame=0 if context_frames is None else max(0,len(prefix)-pending_frames-context_frames)
            audio,sr=self.decode(prefix[start_frame:])
            # Ratio from model config, not a rounded '12 Hz' label (actual 12.5 Hz).
            stride=self.model.speech_tokenizer.model.get_decode_upsample_rate()
            start=emitted-start_frame*stride
            if start < 0 or start > len(audio):
                raise RuntimeError('Unexpected vocoder output length')
            new_audio=audio[start:].copy()
            emitted += len(new_audio)
            pending_frames=0
            yield new_audio,sr,{**timing,'total_codec_frames':len(prefix)}
        self.last_codes=torch.cat(codes,dim=0).clone() if codes else None
        if self.last_codes is None or len(self.last_codes) >= max_tokens:
            raise RuntimeError('Generation did not finish naturally within the token budget')
        # EOS exactly on a codec-chunk boundary may leave an un-emitted group.
        if pending_frames:
            audio,sr=self.decode(self.last_codes)
            yield audio[emitted:].copy(),sr,{**last_info,'is_final':True,
                'total_codec_frames':len(self.last_codes)}

class StreamPlayer:
    """One open WASAPI output; nonblocking producer and measured playback callback.

    PCM is left at its original 24 kHz. WASAPI shared mode performs device sample
    rate conversion. We select the current default endpoint, not a hardcoded ID.
    DAC timestamps are driver estimates; they are not an acoustic measurement.
    """
    def __init__(self,sample_rate=24000):
        import sounddevice as sd
        self.sd=sd
        devices=sd.query_devices()
        hosts=sd.query_hostapis()
        wasapi=next((h for h in hosts if h['name']=='Windows WASAPI'),None)
        device=wasapi['default_output_device'] if wasapi else sd.default.device[1]
        if device is None or device < 0:
            raise RuntimeError('No default audio output device')
        self.device_name=devices[device]['name']
        self.queue=queue.SimpleQueue()
        self.pending=np.zeros(0,dtype=np.float32)
        self.finished=threading.Event()
        self.active=False
        self.ended=True
        self.stream=sd.OutputStream(device=device,samplerate=sample_rate,channels=1,
            dtype='float32',blocksize=0,latency='low',callback=self._callback,
            extra_settings=sd.WasapiSettings(auto_convert=True) if wasapi else None)
        self.sample_rate=sample_rate
        self.stream.start()

    def begin(self,started):
        if not self.ended:
            raise RuntimeError('Previous playback has not drained')
        self.started=started
        self.first_callback_s=self.first_dac_estimate_s=None
        self.underrun_frames=0
        self.status_events=[]
        self.active=False
        self.ended=False
        self.finished.clear()

    def put(self,audio):
        self.queue.put(np.asarray(audio,dtype=np.float32).reshape(-1).copy())

    def _callback(self,outdata,frames,clock,status):
        outdata.fill(0)
        if self.ended:
            return
        if status and self.active:
            self.status_events.append(str(status))
        written=0
        while written < frames:
            if len(self.pending)==0:
                try:
                    item=self.queue.get_nowait()
                except queue.Empty:
                    if self.active:
                        self.underrun_frames += frames-written
                    return
                if item is None:
                    self.ended=True
                    self.finished.set()
                    return
                self.pending=item
                if len(item)==0:
                    continue
            if not self.active:
                self.active=True
                self.first_callback_s=time.perf_counter()-self.started
                self.first_dac_estimate_s=self.first_callback_s+max(0,clock.outputBufferDacTime-clock.currentTime)+written/self.sample_rate
            take=min(frames-written,len(self.pending))
            outdata[written:written+take,0]=self.pending[:take]
            self.pending=self.pending[take:]
            written+=take

    def end(self,timeout=60):
        self.queue.put(None)
        if not self.finished.wait(timeout):
            self.stream.abort()
            raise TimeoutError('Audio output did not drain')
        # Callback completion precedes the device DAC by its output latency.
        time.sleep(self.stream.latency)
        return dict(device=self.device_name,first_callback_s=self.first_callback_s,
            first_dac_estimate_s=self.first_dac_estimate_s,output_latency_s=self.stream.latency,
            underrun_frames=self.underrun_frames,status_events=list(self.status_events))

    def close(self):
        self.stream.stop()
        self.stream.close()
