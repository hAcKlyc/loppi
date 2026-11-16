import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
registerHooks({load(url,context,next){if(url.endsWith('/src/api.ts'))return {format:'module',shortCircuit:true,source:ts.transpileModule(readFileSync(new URL(url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText};return next(url,context);}});
globalThis.window={__TAURI_INTERNALS__:{}};globalThis.isTauri=true;
const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');
const {SpeakerMutation}=await import('../src/api.ts');
const ids=[],receipts=new Map(); let fail=true,creates=0;
mockIPC((_command,args)=>{
 ids.push(args.requestId);
 if(!receipts.has(args.requestId)){creates++;receipts.set(args.requestId,{type:'person_changed',id:args.requestId,person_id:'p'});}
 if(fail){fail=false;throw new Error('response connection lost');}
 return receipts.get(args.requestId);
});
try {
 const mutation=new SpeakerMutation(),request={type:'create_and_bind_speaker',display_name:'Synthetic person',segment_id:'s',expected_revision:1};
 await assert.rejects(mutation.submit(request),/core_transport/);
 const result=await new SpeakerMutation().submit({...request,expected_revision:2});
 assert.equal(result.person_id,'p');assert.equal(creates,1);assert.equal(ids[0],ids[1]);
 await mutation.submit({...request,segment_id:'next'});assert.notEqual(ids[1],ids[2]);
 console.log('PASS: lost confirmation reuses the persisted operation receipt without duplicate people');
} finally {clearMocks();delete globalThis.window;}
