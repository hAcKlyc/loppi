import assert from 'node:assert/strict';
import test from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';
import { desktopPlatform, audioPermissionHelp } from '../src/platform.ts';
import { userError } from '../src/api.ts';
import { setupBlocker, systemAudioPrepared } from '../src/setup.ts';

function withPlatform(platform, body) {
 const descriptor=Object.getOwnPropertyDescriptor(globalThis,'navigator');
 Object.defineProperty(globalThis,'navigator',{configurable:true,value:{platform}});
 try { return body(); } finally {
  if(descriptor)Object.defineProperty(globalThis,'navigator',descriptor);else delete globalThis.navigator;
 }
}

test('desktop presentation distinguishes Windows without changing readiness facts',()=>{
 assert.equal(desktopPlatform('Win32'),'windows');
 assert.equal(desktopPlatform('Win64'),'windows');
 assert.equal(desktopPlatform('MacIntel'),'macos');
 withPlatform('Win32',()=>{
  assert.match(audioPermissionHelp(),/Windows 设置/);
  assert.doesNotMatch(audioPermissionHelp(),/屏幕与系统音频录制/);
  assert.equal(setupBlocker({disk_free_bytes:2e9,audio_used_bytes:0,audio_limit_bytes:1e9,microphone_enabled:false,system_enabled:true},{phase:'ready'},{screen_and_system_audio:0},[]),'请检查或取消选择电脑声音');
  assert.equal(systemAudioPrepared({screen_and_system_audio:0,system_audio:1}),true,'Lockscreen screenshot restriction must not block WASAPI');
  assert.equal(systemAudioPrepared({screen_and_system_audio:1}),false,'A screenshot permission cannot stand in for audio readiness');
 });
 withPlatform('MacIntel',()=>assert.match(audioPermissionHelp(),/屏幕与系统音频录制/));
});

test('native credential errors stay understandable for either secure storage backend',()=>{
 for(const code of ['keychain_save_-25293','credential_save_5'])assert.match(userError(code),/系统安全存储/);
 for(const code of ['keychain_read_-25308','credential_read_1168'])assert.match(userError(code),/重新保存密钥/);
});

test('native screenshot failures explain the desktop constraint without exposing OS details',()=>{
 assert.match(userError('screenshot_desktop_unavailable: WTS'),/解锁 Windows/);
 assert.doesNotMatch(userError('screenshot_desktop_unavailable: WTS'),/WTS/);
 assert.match(userError('screenshot_timeout: private path'),/超时/);
 assert.match(userError('screenshot_window_owner: private PID'),/重新打开入口/);
});

test('real setup groups show Windows device preparation and Windows privacy guidance',async()=>{
 const server=await createServer({configFile:false,server:{middlewareMode:true,hmr:false}});
 try {
  const {SourcePreparation}=await server.ssrLoadModule('/src/SetupGroups.tsx');
  const state={revision:1,microphone_enabled:true,system_enabled:true,microphone_phase:'idle',system_phase:'idle'};
  const privacy={microphone:3,microphone_available:true,tap_required:false,tap_ready:false,screen_and_system_audio:1,system_audio:1};
  const render=(compact,sourcePrivacy=privacy)=>renderToStaticMarkup(createElement(SourcePreparation,{state,privacy:sourcePrivacy,compact,onRefresh:async()=>{}}));
  withPlatform('Win32',()=>{
   const compact=render(true);assert.match(compact,/电脑声音.*已准备/);assert.match(compact,/麦克风.*已授权/);
   const full=render(false);assert.match(full,/Windows 设置/);assert.doesNotMatch(full,/屏幕与系统音频录制/);
   const blocked=render(false,{...privacy,system_audio:0});assert.match(blocked,/待检查/);assert.match(blocked,/检查电脑声音/);
   assert.match(render(true,{...privacy,screen_and_system_audio:0}),/电脑声音.*已准备/);
  });
  withPlatform('MacIntel',()=>{assert.match(render(false),/屏幕与系统音频录制/);assert.match(render(true),/电脑声音.*已授权/)});
 } finally {await server.close()}
});
