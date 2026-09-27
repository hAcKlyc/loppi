use crate::capture::AudioPacket;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Instant, SystemTime};

struct CallbackState {
    sender: mpsc::SyncSender<AudioPacket>,
    dropped: Arc<AtomicU64>,
}

unsafe extern "C" fn receive_audio(
    context: *mut c_void,
    samples: *const f32,
    frames: u32,
    rate: u32,
    channels: u32,
) {
    if context.is_null()
        || samples.is_null()
        || frames == 0
        || rate == 0
        || !(1..=2).contains(&channels)
        || frames > rate * 2
    {
        return;
    }
    let state = unsafe { &*(context as *const CallbackState) };
    let count = frames as usize * channels as usize;
    let packet = AudioPacket {
        at: Instant::now(),
        wall_at: SystemTime::now(),
        samples: unsafe { std::slice::from_raw_parts(samples, count) }.to_vec(),
        rate,
        channels: channels as usize,
    };
    if state.sender.try_send(packet).is_err() {
        state.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct SystemAudio {
    handle: *mut c_void,
    context: *mut CallbackState,
    pub dropped: Arc<AtomicU64>,
}

impl SystemAudio {
    pub fn start(sender: mpsc::SyncSender<AudioPacket>) -> Result<Self, String> {
        unsafe extern "C" {
            fn hear_system_audio_start(
                context: *mut c_void,
                callback: unsafe extern "C" fn(*mut c_void, *const f32, u32, u32, u32),
                stage: *mut i32,
            ) -> *mut c_void;
        }
        let dropped = Arc::new(AtomicU64::new(0));
        let context = Box::into_raw(Box::new(CallbackState {
            sender,
            dropped: Arc::clone(&dropped),
        }));
        let mut stage = 0;
        let handle = unsafe { hear_system_audio_start(context.cast(), receive_audio, &mut stage) };
        if handle.is_null() {
            unsafe { drop(Box::from_raw(context)) };
            return Err(format!("system audio capture failed at stage {stage}"));
        }
        Ok(Self {
            handle,
            context,
            dropped,
        })
    }

    pub fn failed(&self) -> bool {
        unsafe extern "C" {
            fn hear_system_audio_error(handle: *mut c_void) -> i32;
        }
        unsafe { hear_system_audio_error(self.handle) != 0 }
    }
}

impl Drop for SystemAudio {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn hear_system_audio_stop(handle: *mut c_void) -> i32;
        }
        let status = unsafe { hear_system_audio_stop(self.handle) };
        if status == -2 {
            // A timed-out SCStream can still capture and invoke callbacks.
            // The owning helper must exit rather than report this source as
            // stopped while its native stream remains live.
            eprintln!("hear-core system audio stop timed out; terminating capture owner");
            std::process::exit(70);
        } else {
            unsafe { drop(Box::from_raw(self.context)) };
        }
    }
}
