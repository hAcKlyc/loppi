#!/usr/bin/env python3
"""Verify real-model retention/recovery on a supplied public speech fixture.

Writes only temporary isolated databases. Never starts native capture or enables
cloud providers. --models must be an existing model root containing speech-xasr-v1.
The supplied WAV must have voiced speech at 1.0–1.5 s (the published zh fixture
used in the optimization evidence does); otherwise the onset assertion may fail.
"""
import argparse,fcntl,hashlib,json,os,shutil,signal,sqlite3,subprocess,tempfile,time,uuid
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--core',type=Path,required=True)
parser.add_argument('--cli',type=Path,required=True)
parser.add_argument('--models',type=Path,required=True)
parser.add_argument('--wav',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--ffmpeg',default=shutil.which('ffmpeg'))
args=parser.parse_args()
core=args.core.resolve();cli=args.cli.resolve();models=args.models.resolve()
assert core.is_file() and cli.is_file() and args.wav.is_file()
assert (models/'speech-xasr-v1').is_dir() and args.ffmpeg, 'models/ffmpeg unavailable'
phrase=subprocess.check_output([args.ffmpeg,'-v','error','-i',str(args.wav),'-ar','48000','-ac','1','-f','f32le','pipe:1'])
# Public voiced inner phrase begins 200ms before a 5-second block boundary.
phrase=phrase[48000*4:72000*4]*6+phrase[48000*4:]
raw=b'\0'*(int(4.8*48000)*4)+phrase
raw=raw[:15*48000*4].ljust(15*48000*4,b'\0');encoded=[]
for start in range(0,len(raw),240000*4):
 pcm=raw[start:start+240000*4]
 opus=subprocess.check_output([args.ffmpeg,'-v','error','-f','f32le','-ar','48000','-ac','1','-i','pipe:0','-c:a','libopus','-b:a','48k','-frame_duration','20','-f','opus','pipe:1'],input=pcm)
 encoded.append((start//4,len(pcm)//4,opus))
reports={};baseline=None
for case in ['uninterrupted','delayed_close','delayed_crash']:
 with tempfile.TemporaryDirectory(prefix='myagents-verify-hear-delayed-') as tmp:
  data=Path(tmp);env=os.environ|{'HEAR_TEST_PARENT_PID':str(os.getpid()),'HEAR_DATA_DIR':tmp,'HEAR_CORE_BIN':str(core),'HEAR_MODEL_ROOT':str(models)};locks=[];p=None;err=(data/'stderr').open('w')
  def command(*args):return json.loads(subprocess.check_output([str(cli),*args],env=env,text=True,timeout=15))
  def boot():
   p=subprocess.Popen([str(core),'serve','--on-demand'],env=env,stdout=subprocess.DEVNULL,stderr=err,start_new_session=True)
   for _ in range(200):
    if (data/'core.sock').exists():return p
    if p.poll() is not None:raise RuntimeError('core exited')
    time.sleep(.025)
   raise RuntimeError('core socket timeout')
  try:
   p=boot();command('status');command('quit');p.wait(timeout=15)
   anchor=int(time.time()*1000)-60000;db=sqlite3.connect(data/'hear.sqlite')
   for source in ['microphone','system']:
    sid=str(uuid.uuid4());run=str(uuid.uuid4());state='open' if case=='delayed_crash' else 'closed';delayed=case!='uninterrupted'
    db.execute('insert into recording_sessions values(?,?,?,?,?,?)',(sid,source,anchor,anchor+15000,anchor+(5000 if delayed else 15000),state))
    db.execute('insert into session_generation values(?,0)',(sid,));db.execute('insert into session_clock values(?,?,?,0)',(sid,run,anchor));folder=data/'media'/source;folder.mkdir(parents=True)
    if delayed:
     db.execute('insert into ingestion_pending_sessions values(?)',(sid,));journal=data/'ingestion'/sid;journal.mkdir(parents=True)
     fd=os.open(journal,os.O_RDONLY);fcntl.flock(fd,fcntl.LOCK_EX);locks.append(fd)
    for start,frames,opus in encoded:
     id=str(uuid.uuid4());file=folder/f'{id}.opus';file.write_bytes(opus);a=anchor+start//48;b=anchor+(start+frames)//48
     if not delayed or start==0:
      db.execute("insert into audio_blocks(id,session_id,source,started_ms,ended_ms,sample_start,frames,bytes,media_path,retention_decision,analysis_state) values(?,?,?,?,?,?,?,?,?,'pending_vad_v2','pending')",(id,sid,source,a,b,start,frames,len(opus),str(file.relative_to(data))))
     else:
      entry={'version':1,'id':id,'session':{'id':sid,'run_id':run,'source':source,'wall_anchor_ms':anchor,'monotonic_anchor_ns':0,'generation':0},'started_ms':a,'ended_ms':b,'kind':{'kind':'block','sample_start':start,'frames':frames,'digital_silence':False,'bytes':len(opus),'sha256':hashlib.sha256(opus).hexdigest()}}
      (journal/f'{id}.ready').write_text(json.dumps(entry))
   db.commit();db.close();p=boot();begin=time.monotonic()
   for _ in range(120):
    if command('models','status')['state']['phase']=='ready':break
    time.sleep(.05)
   if delayed:
    time.sleep(3)
    db=sqlite3.connect(data/'hear.sqlite');held=db.execute('select source,analysis_state,retention_decision,media_path is not null,analysis_attempts from audio_blocks order by source').fetchall();db.close()
    assert len(held)==2 and all(row[1:] == ('pending','pending_vad_v2',1,0) for row in held),held
    for fd in locks:os.close(fd)
    locks=[]
   else:held=[]
   terminal=False
   while time.monotonic()-begin<90:
    time.sleep(.05);db=sqlite3.connect(data/'hear.sqlite');blocks=db.execute('select analysis_state,count(*) from audio_blocks group by analysis_state').fetchall();jobs=db.execute('select state,count(*) from coarse_jobs group by state').fetchall();db.close()
    if sum(n for s,n in blocks)==6 and all(s in ('complete','not_needed') for s,n in blocks) and jobs and all(s=='complete' for s,n in jobs):terminal=True;break
   assert terminal,(case,blocks,jobs)
   db=sqlite3.connect(data/'hear.sqlite');segments={source:db.execute('select started_ms-?,ended_ms-?,text from coarse_segments where source=? order by started_ms,ended_ms',(anchor,anchor,source)).fetchall() for source in ['microphone','system']};retention=db.execute('select source,sample_start,retention_decision,media_path is not null from audio_blocks order by source,sample_start').fetchall();db.close()
   if baseline is None:
    assert all(row[-1]==1 for row in retention if row[1]==0),retention
    baseline=(segments,retention)
   assert (segments,retention)==baseline,(case,retention,baseline[1])
   reports[case]={'seconds':time.monotonic()-begin,'audio_seconds_per_source':15,'onset_ms':4800,'held_before_next_import':held,'blocks':blocks,'jobs':jobs,'segments_per_source':{s:len(v) for s,v in segments.items()},'retention':retention,'same_as_uninterrupted':True}
   print(json.dumps({case:reports[case]}),flush=True)
   command('quit');p.wait(timeout=15)
  finally:
   for fd in locks:os.close(fd)
   if p is not None and p.poll() is None:
    try: command('quit');p.wait(timeout=15)
    except (OSError,subprocess.SubprocessError):os.killpg(p.pid,signal.SIGKILL);p.wait()
   err.close()
args.output.write_text(json.dumps({'scope':'Public voiced inner phrase onset at 4.8s; two sources; actual models; delayed journal import held 3s after model-ready; no real holdout or native capture claim','core_sha256':hashlib.sha256(core.read_bytes()).hexdigest(),'worker_sha256':hashlib.sha256((core.parent/'hear-worker').read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(args.wav.read_bytes()).hexdigest(),'cases':reports},indent=2)+'\n')
