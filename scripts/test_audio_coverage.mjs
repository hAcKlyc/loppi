import assert from 'node:assert/strict';
import { test } from 'node:test';
import { audioCoverageLabel, processingLabel } from '../src/api.ts';

test('completed transcription and exact source-specific historical gaps coexist', () => {
 const item = { processing_state: 'transcribed', audio_coverage: [{source:'microphone',duration_ms:164363,interval_count:2,reasons:['machine_sleep','source_start_gap']}] };
 assert.match(processingLabel(item.processing_state), /完成/);
 assert.equal(audioCoverageLabel(item), '麦克风：2 段录音缺口，共 164.363 秒（电脑休眠、恢复启动）');
 assert.equal(audioCoverageLabel({}), '');
});

test('known capture failures retain their cause', () => {
 const label = audioCoverageLabel({audio_coverage:[{source:'system',duration_ms:1000,interval_count:1,reasons:['callback_gap','interrupted_seal']}]});
 assert.match(label,/音频采集丢失、音频封存中断/);
 assert.doesNotMatch(label,/未记录/);
});
