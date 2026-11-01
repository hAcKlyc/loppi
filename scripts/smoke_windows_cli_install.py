"""Real Windows CLI launcher idempotence, cold Core lookup, stdio and conflict preservation."""
import json,os,subprocess,uuid
from pathlib import Path
root=Path.cwd();runtime=root/'build/windows-dev-runtime';directory=root/'build/windows-cli-install-test'/('命令 🐰 space '+str(uuid.uuid4()));directory.mkdir(parents=True)
env={k:v for k,v in os.environ.items() if k!='LOPPI_CORE_BIN'};env.update(LOPPI_DATA_DIR=str(directory/'data'),LOPPI_MODEL_ROOT=str(root/'build/cache/windows/models'),LOPPI_TEST_PARENT_PID=str(os.getpid()))
report={'schema':1,'checks':[]}
def command(binary,args,stdin=None,expected=0):
    result=subprocess.run([str(binary),*args],env=env,input=stdin,capture_output=True,encoding='utf8',timeout=45);assert result.returncode==expected,(args,result.returncode,result.stderr,result.stdout);return result.stdout
try:
    destination=directory/'command';original=runtime/'loppi.exe'
    first=json.loads(command(original,['integrations','install-cli','--directory',str(destination)]));second=json.loads(command(original,['integrations','install-cli','--directory',str(destination)]));assert first==second
    installed=destination/'loppi.exe';state=json.loads(command(installed,['status']));assert state['type']=='status'
    command(installed,['quit']);report['checks'].append({'name':'install-repeat-cold-core-lookup','passed':True})
    frames=[{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'synthetic','version':'1'}}},{'jsonrpc':'2.0','method':'notifications/initialized'},{'jsonrpc':'2.0','id':2,'method':'tools/list'}]
    output=command(installed,['mcp','serve'],''.join(json.dumps(frame)+'\n' for frame in frames));assert len(json.loads(output.splitlines()[-1])['result']['tools'])==7
    report['checks'].append({'name':'launcher-preserves-mcp-stdio','passed':True})
    conflict=directory/'conflict';conflict.mkdir();sentinel=conflict/'loppi.exe';sentinel.write_bytes(b'synthetic unrelated file')
    result=subprocess.run([str(original),'integrations','install-cli','--directory',str(conflict)],env=env,capture_output=True,timeout=30);assert result.returncode!=0;assert sentinel.read_bytes()==b'synthetic unrelated file'
    report['checks'].append({'name':'unrelated-file-preserved','passed':True});report['passed']=True
finally:
    (directory.parent/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf8')
print(json.dumps(report,indent=2))
