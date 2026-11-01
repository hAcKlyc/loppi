"""Real dual-source pause/resume and seven Agent operations, isolated Unicode data.

Seeds only disposable queue metadata while Core is stopped. The existing test
owner blocks credential reads/network calls; this does not validate cloud APIs.
"""
import argparse,datetime,json,os,sqlite3,subprocess,time,uuid,winsound
from pathlib import Path

parser=argparse.ArgumentParser();parser.add_argument('--runtime',type=Path,default=Path('build/windows-dev-runtime'));args=parser.parse_args()
root=Path.cwd();runtime=args.runtime.resolve();directory=root/'build/windows-lifecycle-test';directory.mkdir(parents=True,exist_ok=True)
models=runtime/'resources/models' if (runtime/'resources/models').is_dir() else root/'build/cache/windows/models'
data=directory/('隔离 data '+str(uuid.uuid4()));env=os.environ.copy();env.update(LOPPI_DATA_DIR=str(data),LOPPI_MODEL_ROOT=str(models),LOPPI_TEST_PARENT_PID=str(os.getpid()))
report={'schema':1,'real_windows':True,'cloud_api_requests':False,'fixture':'disposable configured+paused queues; existing offline test owner','data':str(data),'checks':[]}
owner=None
def command(*args,allow_error=False):
    result=subprocess.run([str(runtime/'loppi.exe'),*args],env=env,capture_output=True,encoding='utf8',timeout=90)
    if result.returncode and not allow_error:raise RuntimeError(f'{args}: {result.stderr}')
    return json.loads(result.stdout)
def start():return subprocess.Popen([str(runtime/'loppi-core.exe'),'serve','--on-demand'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def wait(predicate,timeout=60):
    deadline=time.monotonic()+timeout
    while True:
        state=command('status')['state']
        if predicate(state):return state
        if time.monotonic()>deadline:raise RuntimeError(f'state timeout: {state}')
        time.sleep(.25)
try:
    owner=start();command('status');command('quit');owner.wait(timeout=15);assert owner.returncode==0
    db=sqlite3.connect(data/'loppi.sqlite');providers=[row[0] for row in db.execute('select provider from cloud_queues')];assert len(providers)==2
    for provider in providers:
        db.execute('update cloud_queues set configured=1,paused=1 where provider=?',(provider,))
        db.execute('insert into cloud_credential_refs(provider,account) values (?,?)',(provider,'synthetic:'+str(uuid.uuid4())))
    db.commit();db.close();owner=start()
    deadline=time.monotonic()+90
    while command('models','status')['state']['phase']!='ready':
        if time.monotonic()>deadline:raise RuntimeError('offline models timeout')
        time.sleep(1)
    synthetic=root/'build/windows-core-acceptance/synthetic-english.wav';assert synthetic.is_file(),'run smoke_windows_core.py first to create local synthetic SAPI speech'
    winsound.PlaySound(str(synthetic),winsound.SND_FILENAME|winsound.SND_ASYNC|winsound.SND_LOOP)
    command('start');recording=wait(lambda s:s['microphone_phase']=='recording' and s['system_phase']=='recording')
    time.sleep(6);command('pause');paused=wait(lambda s:s['microphone_phase'] in ['idle','disabled'] and s['system_phase'] in ['idle','disabled'])
    assert paused['paused_intent'];time.sleep(2)
    db=sqlite3.connect(data/'loppi.sqlite');before=db.execute('select count(*) from audio_blocks').fetchone()[0];time.sleep(2);assert db.execute('select count(*) from audio_blocks').fetchone()[0]==before
    command('resume');resumed=wait(lambda s:s['microphone_phase']=='recording' and s['system_phase']=='recording');time.sleep(6)
    command('pause');wait(lambda s:s['microphone_phase']=='idle' and s['system_phase']=='idle');time.sleep(2)
    after=db.execute('select count(*) from audio_blocks').fetchone()[0];assert after>before>0,(before,after)
    gaps=db.execute("select source,started_ms,ended_ms,reason from media_gaps where reason='manual_pause'").fetchall();assert any(row[2] is not None for row in gaps),gaps
    report['checks'].append({'name':'actual-dual-source-pause-resume','recording':recording,'paused':paused,'resumed':resumed,'blocks_before':before,'blocks_after':after,'gaps':gaps})
    # Observe the actual persisted capture -> VAD -> X-ASR -> current read path.
    deadline=time.monotonic()+150
    while True:
        coarse=db.execute("select id,source,started_ms,ended_ms,text,model_version from coarse_segments where source='system' order by started_ms").fetchall()
        if any('WINDOWS' in row[4].upper() or 'TESTING' in row[4].upper() for row in coarse):break
        if time.monotonic()>deadline:raise RuntimeError({'coarse':coarse,'analysis':db.execute('select source,analysis_state,count(*) from audio_blocks group by 1,2').fetchall()})
        time.sleep(1)
    media=db.execute("select source,started_ms,ended_ms,frames,media_path,analysis_state from audio_blocks where source='system' and media_path is not null order by started_ms").fetchall();assert media
    iso=lambda value:datetime.datetime.fromtimestamp(value/1000,datetime.timezone.utc).isoformat()
    current=command('context','read','--from',iso(media[0][1]-1000),'--to',iso(media[-1][2]+1000),'--sources','system')
    assert current['ok'] and any(word in json.dumps(current).upper() for word in ['WINDOWS','TESTING']),current
    report['checks'].append({'name':'persisted-system-capture-vad-xasr-current-cli-read','coarse':coarse,'media':media,'current_context':current,'model_root':str(models)})
    operations=[['status'],['list'],['digest'],['search','synthetic'],['event',str(uuid.uuid4())],['read'],['people']]
    for arguments in operations:
        value=command('context',*arguments,allow_error=True);assert isinstance(value.get('ok'),bool),value
        report['checks'].append({'name':'cli-context-'+arguments[0],'ok':value['ok'],'error':value.get('error')})
    frames=[{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'synthetic-test','version':'1'}}},{'jsonrpc':'2.0','method':'notifications/initialized'},{'jsonrpc':'2.0','id':2,'method':'tools/list'}]
    result=subprocess.run([str(runtime/'loppi.exe'),'mcp','serve'],env=env,input=''.join(json.dumps(value)+'\n' for value in frames),capture_output=True,encoding='utf8',timeout=30);assert result.returncode==0,result.stderr
    responses=[json.loads(line) for line in result.stdout.splitlines()];tools=responses[-1]['result']['tools'];assert len(tools)==7,tools
    report['checks'].append({'name':'actual-mcp-stdio-handshake-seven-tools','tools':[item['name'] for item in tools]})
    command('resume');wait(lambda s:s['microphone_phase']=='recording' and s['system_phase']=='recording');time.sleep(1)
    stopped=command('prepare-install');assert stopped=={'stopped':True,'forced':False},stopped
    owner.wait(timeout=15);assert owner.returncode==0
    report['checks'].append({'name':'normal-install-preparation-during-dual-recording','response':stopped});report['passed']=True
finally:
    winsound.PlaySound(None,0)
    if owner and owner.poll() is None:
        try:command('quit');owner.wait(timeout=15)
        except Exception:owner.kill();owner.wait()
    (directory/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
print(json.dumps(report,ensure_ascii=True,indent=2))
