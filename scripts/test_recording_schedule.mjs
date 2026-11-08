import assert from 'node:assert/strict';
import {test} from 'node:test';
import {scheduleTime,scheduleMinute,nextScheduleAction,scheduleSummary} from '../src/recording-schedule.ts';
import {captureStatusView} from '../src/capture-status.ts';

test('time controls round trip every minute and reject incomplete or invalid input',()=>{
 for(let m=0;m<1440;m++) assert.equal(scheduleMinute(scheduleTime(m)),m);
 for(const v of ['', '24:00','23:60','-1:00','8:00','08:1']) assert.equal(scheduleMinute(v),null);
});
test('schedule presentation uses actual capture and reports the next plan separately',()=>{
 const schedule={enabled:true,start_minute:480,stop_minute:1380,paused_by_schedule:true,next_at_ms:new Date(2026,9,21,8).getTime(),next_action:'start',error:null};
 assert.match(nextScheduleAction(schedule),/08:00.*开始录制/);
 assert.match(nextScheduleAction({...schedule,error:'schedule_clock_unavailable'}),/检查系统时间/);
 const snapshot={appearance:'sleeping',control:'resume',error:null,state:{paused_intent:true,recording_schedule:schedule}};
 assert.equal(captureStatusView(snapshot).label,'定时暂停');
 assert.match(captureStatusView(snapshot).subtitle,/开始录制/);
 assert.equal(captureStatusView({...snapshot,appearance:'listening',state:{...snapshot.state,paused_intent:false,microphone_recording:true}}).label,'正在记录');
 assert.equal(captureStatusView({...snapshot,appearance:'interrupted',state:{...snapshot.state,storage_paused:true}}).label,'空间不足');
});
test('weekday summary preserves legacy daily and marks overnight stops',()=>{
 const day={start_minute:480,stop_minute:1380};
 assert.equal(scheduleSummary(day),'每天 · 08:00–23:00');
 assert.equal(scheduleSummary({...day,weekday_mask:31}),'周一至周五 · 08:00–23:00');
 assert.equal(scheduleSummary({...day,weekday_mask:96}),'周末 · 08:00–23:00');
 assert.equal(scheduleSummary({start_minute:1320,stop_minute:360,weekday_mask:16}),'周五 · 22:00–次日 06:00');
});
