"""Real GPU worker lifecycle checks; only fixed synthetic test phrases are spoken."""
import json,pathlib,subprocess,threading,queue,time,os
ROOT=pathlib.Path(__file__).resolve().parent
cfg=json.loads((ROOT/'runtime.json').read_text(encoding='utf-8-sig'))
log=[]
def launch():
    p=subprocess.Popen([cfg['python'],'-u','-X','utf8',str(ROOT/'worker.py')],
        stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(ROOT/f'worker-test-stderr-{os.getpid()}.log').open('w',encoding='utf8'),
        text=True,encoding='utf8',creationflags=0x08000000)
    events=queue.Queue()
    def read():
        for line in p.stdout:
            event=json.loads(line);events.put(event);log.append(event)
            print(event,flush=True)
    threading.Thread(target=read,daemon=True).start()
    return p,events
def send(p,**data):
    p.stdin.write(json.dumps(data,ensure_ascii=False)+'\n');p.stdin.flush()
def wait(q,kind,request=None,timeout=90):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        e=q.get(timeout=max(.01,end-time.monotonic()))
        assert e['event'] not in ('fatal','error'),e
        if e['event']==kind and (request is None or e['id']==request): return e
    raise TimeoutError(kind)
p,q=launch()
try:
    wait(q,'loading')
    send(p,cmd='speak',id=1,text='此句必须被取消。')
    send(p,cmd='stop',id=2)
    wait(q,'ready')
    wait(q,'stopped',2)
    assert not any(e['event']=='started' and e['id']==1 for e in log)
    send(p,cmd='speak',id=3,text='需要确认一下，你说的是访问令牌过期，还是刷新令牌过期？')
    wait(q,'started',3)
    send(p,cmd='stop',id=4)
    wait(q,'stopped',4,timeout=3)
    send(p,cmd='speak',id=5,text='已整理完成。')
    wait(q,'started',5)
    done=wait(q,'done',5)
    assert done['underrun_frames']==0 and done['driver_errors']==0,done
    assert not any(e['event']=='done' and e['id']==3 for e in log)
    send(p,cmd='shutdown')
    p.wait(timeout=5)
finally:
    if p.poll() is None:p.kill();p.wait()
# Standalone EOF is drained after initialization. The Rust Job Object separately
# guarantees immediate cleanup even if the app quits during model loading.
p,q=launch()
try:
    wait(q,'loading',timeout=5)
    p.stdin.close()
    p.wait(timeout=30)
finally:
    if p.poll() is None:p.kill();p.wait()
(ROOT/'worker-test-results.json').write_text(json.dumps(log,indent=2),encoding='utf8')
print('PASS: startup cancellation, live interruption, replacement receipt, graceful shutdown, parent EOF during loading')
