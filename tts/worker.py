"""Local stdio-only TTS worker. Protocol stdout never contains text or credentials."""
import sys,os,json,time,threading,pathlib,queue,ctypes
ROOT=pathlib.Path(__file__).resolve().parent
sys.path.insert(0,str(ROOT))
protocol=sys.stdout
sys.stdout=sys.stderr  # Third-party model progress must not enter the JSON protocol.
write_lock=threading.Lock()
condition=threading.Condition(threading.RLock())
latest=0
pending=None
player=None
def event(kind,request=0,**fields):
    with write_lock:
        protocol.write(json.dumps(dict(event=kind,id=request,**fields),ensure_ascii=False)+'\n')
        protocol.flush()
def read_commands():
    global latest,pending
    try:
        for line in sys.stdin:
            command=json.loads(line)
            kind=command.get('cmd')
            if kind=='shutdown': os._exit(0)
            if kind not in ('speak','stop'): continue
            with condition:
                latest=int(command['id'])
                pending=None
                if player is not None: player.cancel()
                if kind=='speak':
                    text=command.get('text','').strip()
                    if not text or len(text)>500:
                        event('error',latest,message='播报内容为空或过长，请查看文字结果。')
                    else:
                        pending=(latest,text,time.monotonic())
                condition.notify_all()
            if kind=='stop': event('stopped',latest)
    except Exception:
        pass
    # stdin EOF means the parent disappeared, including while model imports/load block.
    os._exit(0)

# The UI also reads the machine-wide QPC. Send the callback timestamp via
# a queue so a blocked stdout pipe can never stall the realtime audio thread.
output_marks = queue.SimpleQueue()
query_counter = ctypes.windll.kernel32.QueryPerformanceCounter

def report_audio_output():
    first_playback = True
    while True:
        request, counter = output_marks.get()
        event('first_output', request, counter=counter, first_playback=first_playback)
        first_playback = False

class Player:
    def __init__(self):
        import numpy as np,sounddevice as sd
        from audio_output import DefaultOutput
        self.np=np;self.lock=threading.RLock();self.queue=queue.SimpleQueue()
        self.pending=np.zeros(0,dtype='float32');self.id=0;self.active=False
        self.output_marked=False;self.counter=ctypes.c_int64()
        self.finished=threading.Event();self.underruns=0;self.driver_errors=0
        self.output=DefaultOutput(sd, self.callback)
    def cancel(self):
        with self.lock:
            self.id=0;self.active=False;self.queue=queue.SimpleQueue()
            self.pending=self.np.zeros(0,dtype='float32');self.finished.set()
    def begin(self,request):
        self.cancel()
        with self.lock:
            self.output_marked=False
            self.id=request;self.underruns=0;self.driver_errors=0;self.finished.clear()
    def put(self,request,audio):
        with self.lock:
            if request==self.id:
                self.queue.put(audio.copy() if audio is not None else None)
    def callback(self,out,frames,clock,status):
        out.fill(0)
        with self.lock:
            if not self.id: return
            if status and self.active: self.driver_errors+=1
            written=0
            while written<frames:
                if not len(self.pending):
                    try: a=self.queue.get_nowait()
                    except queue.Empty:
                        if self.active: self.underruns+=frames-written
                        return
                    if a is None:
                        self.id=0;self.active=False;self.finished.set();return
                    self.pending=a
                    if not len(a): continue
                self.active=True
                n=min(frames-written,len(self.pending))
                out[written:written+n,0]=self.pending[:n]
                if not self.output_marked and self.np.any(self.np.abs(self.pending[:n]) > 0.001):
                    if query_counter(ctypes.byref(self.counter)):
                        output_marks.put((self.id, self.counter.value))
                        self.output_marked = True
                self.pending=self.pending[n:];written+=n
    def close(self):
        self.cancel();self.output.close()

try:
    event('loading')
    from qwen_stream import QwenStream
    event('model_loading')
    config=json.loads((ROOT/'runtime.json').read_text(encoding='utf-8-sig'))
    model=pathlib.Path(config['model'])
    if not model.is_absolute(): model=ROOT/model
    style=config.get('style_instruction')
    if style is not None and not isinstance(style,str): raise ValueError('Invalid voice style')
    engine=QwenStream(model,instruct=style or None)
    if engine.model.model.model.tts_model_size!='1b7': raise ValueError('Expected 1.7B')
    event('warming')
    engine.warmup()
    event('opening_audio')
    threading.Thread(target=report_audio_output, daemon=True).start()
    with condition: player=Player()
    # Open the stream during warmup when possible. Missing output can recover
    # on the next request as soon as the user plugs in a playback device.
    try:
        player.output.ensure()
        output_available = True
    except Exception:
        output_available = False
    event('ready', output_available=output_available)
except Exception:
    event('fatal',message='Qwen 语音加载失败，请检查本地运行环境、模型和输出设备。')
    raise SystemExit(1)
# Initialize all native libraries before a blocking CRT stdin read. Starting
# the reader before NumPy import deadlocks its DLL initialization on this runtime.
# Commands queue in the pipe while loading; the parent's Job Object owns lifetime.
threading.Thread(target=read_commands,daemon=True).start()
while True:
    with condition:
        condition.wait_for(lambda: pending is not None)
        request,text,created=pending
        pending=None
        if time.monotonic()-created>30:
            event('error',request,message='播报等待过久，已跳过；文字结果仍保留。');continue
    started=time.perf_counter();first=True
    try:
        player.output.ensure()
        with condition:
            if request!=latest: continue
            player.begin(request)
        for audio,sr,info in engine.stream(text):
            with condition:
                if request!=latest: break
                player.put(request,audio)
                if first:
                    event('started',request,first_pcm_ms=round((time.perf_counter()-started)*1000,1))
                    first=False
        with condition:
            if request!=latest: continue
            player.put(request,None)
        while not player.finished.wait(.02):
            if request!=latest: break
            if time.perf_counter()-started>90: raise TimeoutError()
        with condition:
            if request==latest:
                event('done',request,underrun_frames=player.underruns,driver_errors=player.driver_errors)
    except Exception:
        with condition:
            if request==latest:
                player.cancel()
                event('error',request,message='语音播报失败，文字结果仍保留。可在设置中重启语音。')
