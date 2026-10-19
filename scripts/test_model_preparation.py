#!/usr/bin/env python3
"""Isolated Core load-check integration. No capture, keys or cloud calls.
Usage: python3 scripts/test_model_preparation.py /path/to/existing/models [core binary]
"""
import json, os, pathlib, re, shutil, socket, sqlite3, subprocess, sys, tempfile, time, uuid
root=pathlib.Path(__file__).resolve().parent.parent
args=[v for v in sys.argv[2:] if v!='--split']; split='--split' in sys.argv
binary=pathlib.Path(args[0]) if args else root/'target/debug/hear-core'
models=pathlib.Path(sys.argv[1])
version=int(re.search(r'PROTOCOL_VERSION: u32 = (\d+)',(root/'crates/hear-protocol/src/lib.rs').read_text())[1])
with tempfile.TemporaryDirectory(prefix='hear-onboarding-model-') as tmp:
 directory=pathlib.Path(tmp)
 # Copy only pinned models; never link user data that the test could mutate.
 shutil.copytree(models,directory/'models')
 if split:
  staging=directory/'models/.installing-speech-v1';staging.mkdir()
  (directory/'models/speech-v1/silero_vad.onnx').rename(staging/'silero_vad.onnx')
 process=subprocess.Popen([str(binary),'serve','--on-demand'],env=os.environ|{'HEAR_DATA_DIR':tmp},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 def query(request):
  with socket.socket(socket.AF_UNIX) as s:
   s.settimeout(8);s.connect(str(directory/'core.sock'));s.sendall(json.dumps({'version':version,'id':str(uuid.uuid4()),'client':'cli','request':request}).encode());s.shutdown(socket.SHUT_WR)
   chunks=[]
   while chunk:=s.recv(65536):chunks.append(chunk)
   return json.loads(b''.join(chunks))
 try:
  deadline=time.monotonic()+100
  while not (directory/'core.sock').exists() and time.monotonic()<deadline:time.sleep(.1)
  if split:
   while query({'type':'models'})['state']['phase']=='checking':time.sleep(.1)
   query({'type':'prepare_models'})
  observed=set()
  while time.monotonic()<deadline:
   result=query({'type':'models'});assert result['type']=='models',result
   state=result['state'];observed.add(state['phase'])
   if state['phase'] in ('ready','failed'):break
   time.sleep(.2)
  assert state['phase']=='ready',state
  if split:assert 'downloading' not in observed,observed
  status=query({'type':'status'})['state'];assert not status['enabled_intent'];assert not status['microphone_recording'];assert not status['system_recording']
  receipt=query({'type':'cloud_configuration_receipt','provider':'llm','request_id':str(uuid.uuid4())});assert receipt['response'] is None and not receipt['pending'],receipt
  db=sqlite3.connect(directory/'hear.sqlite')
  for table in ('recording_sessions','audio_blocks','coarse_segments','cloud_tasks'):
   assert db.execute('SELECT count(*) FROM '+table).fetchone()[0]==0,table
  db.close()
  print('PASS: actual VAD/SenseVoice load, stages',sorted(observed),'split component reuse' if split else 'whole pack', 'no intent/capture/business data/cloud tasks; receipt query read-only')
 finally:
  process.terminate();process.wait(timeout=10)
