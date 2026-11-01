"""Real Windows audio/inference acceptance using disposable data and local synthetic speech."""
import argparse, json, os, sqlite3, subprocess, time, winsound, base64, io, wave, array, uuid
from windows_ipc import request
from pathlib import Path

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--runtime',type=Path,default=Path('build/windows-dev-runtime'));args=parser.parse_args()
    runtime=args.runtime.resolve();root=Path.cwd();directory=root/'build/windows-core-acceptance';directory.mkdir(parents=True,exist_ok=True)
    data=directory/('data-'+str(uuid.uuid4()));env=os.environ.copy();env.update(LOPPI_DATA_DIR=str(data),LOPPI_MODEL_ROOT=str(root/'build/cache/windows/models'),LOPPI_TEST_PARENT_PID=str(os.getpid()))
    cli=runtime/'loppi.exe';report={'schema':1,'real_windows':True,'paid_api_requests':False,'data':str(data),'checks':[]}
    def command(*arguments,timeout=30):
        result=subprocess.run([str(cli),*map(str,arguments)],env=env,capture_output=True,text=True,encoding='utf8',timeout=timeout)
        if result.returncode:raise RuntimeError(f'{arguments}: {result.returncode}: {result.stderr}')
        return json.loads(result.stdout)
    owner=subprocess.Popen([str(runtime/'loppi-core.exe'),'serve','--on-demand'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        deadline=time.monotonic()+90
        while True:
            state=command('models','status')['state']
            if state['phase']=='ready' and state['speaker_phase']=='ready':break
            if state['phase']=='failed' or time.monotonic()>deadline:raise RuntimeError(f'model readiness: {state}')
            time.sleep(1)
        report['checks'].append({'name':'verified-offline-models','state':state})
        for source in ['microphone','system']:
            report['checks'].append({'name':f'{source}-probe','response':command('probe',source,'--seconds',2)})
        wav=directory/'synthetic-english.wav';tts_env=env|{'LOPPI_SYNTHETIC_WAV':str(wav)}
        script="Add-Type -AssemblyName System.Speech; $speech=New-Object System.Speech.Synthesis.SpeechSynthesizer; $speech.SelectVoice('Microsoft Zira Desktop'); $speech.SetOutputToWaveFile($env:LOPPI_SYNTHETIC_WAV); $speech.Speak('Today we are testing the Windows audio recorder. The microphone and computer audio should use the same clock. This is synthetic speech created on this computer. No private recording is used.'); $speech.Dispose()"
        subprocess.run(['powershell.exe','-NoProfile','-Command',script],env=tts_env,check=True,capture_output=True,timeout=30)
        asr=subprocess.run([str(runtime/'loppi-worker.exe'),'self-test',str(root/'build/cache/windows/models/speech-xasr-v1'),str(wav)],env=env,capture_output=True,text=True,encoding='utf8',timeout=90)
        if asr.returncode or not asr.stdout.strip():raise RuntimeError(f'offline ASR: {asr.returncode}: {asr.stderr[-1200:]}')
        report['checks'].append({'name':'offline-synthetic-speech-recognition','text':asr.stdout.strip()})
        began=int(time.time()*1000)
        recordings={source:subprocess.Popen([str(cli),'record-test',source,'--seconds','10'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE) for source in ['microphone','system']}
        time.sleep(.75);winsound.PlaySound(str(wav),winsound.SND_FILENAME|winsound.SND_ASYNC|winsound.SND_LOOP)
        try:
            for source,process in recordings.items():
                out,error=process.communicate(timeout=30)
                if process.returncode:raise RuntimeError(f'{source}: {error.decode("utf8",errors="replace")}')
                response=json.loads(out);assert response['type']=='record_test_result' and response['frames']>400000,response
                report['checks'].append({'name':f'{source}-recording','response':response})
        finally:winsound.PlaySound(None,0)
        ended=int(time.time()*1000)
        # Observe only this test database. Actual source supervisors wrote all recording facts.
        db=sqlite3.connect(data/'loppi.sqlite');db.row_factory=sqlite3.Row
        blocks=[dict(row) for row in db.execute('SELECT source,started_ms,ended_ms,sample_start,frames,media_path,analysis_state FROM audio_blocks ORDER BY source,sample_start')]
        clocks=[dict(row) for row in db.execute('SELECT * FROM session_clock')]
        assert len(clocks)>=2,(clocks,blocks)
        for row in blocks:
            assert began-2000<=row['started_ms']<=ended+2000,row
            assert abs((row['ended_ms']-row['started_ms'])-row['frames']/48)<=2,row
            if row['media_path']:assert '\\' not in row['media_path'],row
        system_block=next(row for row in blocks if row['source']=='system' and row['media_path'])
        playback=request(data,{'type':'playback_chunk','from_ms':system_block['started_ms'],'to_ms':system_block['ended_ms'],'sources':'system'})
        assert playback['type']=='playback_chunk' and playback['has_audio'],playback
        with wave.open(io.BytesIO(base64.b64decode(playback['wav_base64'])),'rb') as audio:
            samples=array.array('h',audio.readframes(audio.getnframes()))
        peak=max(abs(sample) for sample in samples)/32768
        assert peak>.001,peak
        report['checks'].append({'name':'captured-system-speech-playback','peak':peak,'samples':len(samples)})
        report['checks'].append({'name':'dual-source-time-coordinates' ,'blocks':blocks,'clocks':clocks,'wall_range':[began,ended]})
        report['checks'].append({'name':'normal-core-quit','response':command('quit')})
        owner.wait(timeout=15);assert owner.returncode==0,owner.returncode
        report['passed']=True
    finally:
        winsound.PlaySound(None,0)
        if owner.poll() is None:
            try:command('quit',timeout=15);owner.wait(timeout=15)
            except Exception:owner.kill();owner.wait()
        (directory/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    print(json.dumps(report,ensure_ascii=True,indent=2))

if __name__=='__main__':main()
