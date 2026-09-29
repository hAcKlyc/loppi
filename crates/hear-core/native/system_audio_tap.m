#import <Foundation/Foundation.h>
#import <CoreAudio/AudioHardware.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#include <math.h>
#include <dlfcn.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

// Resolve the 14.2 functions only on a supported OS. A direct Mach-O import
// would make the macOS 13 ScreenCaptureKit fallback fail before main runs.
typedef OSStatus (*HearCreateTap)(CATapDescription *, AudioObjectID *);
typedef OSStatus (*HearDestroyTap)(AudioObjectID);

static HearCreateTap hear_create_tap_function(void) {
  return (HearCreateTap)dlsym(RTLD_DEFAULT, "AudioHardwareCreateProcessTap");
}

static HearDestroyTap hear_destroy_tap_function(void) {
  return (HearDestroyTap)dlsym(RTLD_DEFAULT, "AudioHardwareDestroyProcessTap");
}

typedef void (*HearAudioCallback)(void *, const float *, uint32_t, uint32_t, uint32_t);

extern int hear_allow_audio_idle_sleep(void);
extern void *hear_legacy_system_audio_start(void *, HearAudioCallback, int *);
extern int hear_legacy_system_audio_error(void *);
extern int hear_legacy_system_audio_stop(void *);
extern int64_t hear_legacy_system_audio_probe(uint32_t, uint64_t *);

typedef struct {
  AudioObjectID tap;
  AudioObjectID device;
  AudioDeviceIOProcID io_proc;
  uint32_t sample_rate;
  HearAudioCallback callback;
  void *callback_context;
  atomic_uint_fast64_t last_frame_ns;
  atomic_int format_error;
  bool started;
} HearTapSession;

typedef struct {
  bool uses_tap;
  void *session;
} HearSystemSession;

static uint64_t monotonic_ns(void) {
  struct timespec now;
  clock_gettime(CLOCK_MONOTONIC, &now);
  return (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec;
}

static OSStatus hear_tap_receive(AudioObjectID device, const AudioTimeStamp *now,
                                  const AudioBufferList *input, const AudioTimeStamp *input_time,
                                  AudioBufferList *output, const AudioTimeStamp *output_time,
                                  void *context) {
  (void)device;
  (void)now;
  (void)input_time;
  (void)output;
  (void)output_time;
  HearTapSession *session = context;
  if (!input || input->mNumberBuffers != 1 || !input->mBuffers[0].mData ||
      input->mBuffers[0].mNumberChannels != 1 ||
      input->mBuffers[0].mDataByteSize % sizeof(float) != 0) {
    atomic_store(&session->format_error, 1);
    return noErr;
  }
  const AudioBuffer *buffer = &input->mBuffers[0];
  uint32_t frames = buffer->mDataByteSize / sizeof(float);
  if (frames > 0) {
    atomic_store(&session->last_frame_ns, monotonic_ns());
    session->callback(session->callback_context, buffer->mData, frames,
                      session->sample_rate, 1);
  }
  return noErr;
}

// A successful AudioDeviceStop drains the IOProc before its Rust callback
// context is released. If HAL cannot stop/destroy it, keep the session alive
// and let the capture owner terminate instead of risking a callback into freed
// memory.
static int hear_tap_stop(HearTapSession *session) {
  if (session->started) {
    OSStatus status = AudioDeviceStop(session->device, session->io_proc);
    if (status != noErr) return -2;
    session->started = false;
  }
  if (session->io_proc) {
    OSStatus status = AudioDeviceDestroyIOProcID(session->device, session->io_proc);
    if (status != noErr) return -2;
    session->io_proc = NULL;
  }
  int result = 0;
  if (session->device != kAudioObjectUnknown) {
    if (AudioHardwareDestroyAggregateDevice(session->device) != noErr) result = -3;
    session->device = kAudioObjectUnknown;
  }
  if (session->tap != kAudioObjectUnknown) {
    HearDestroyTap destroy_tap = hear_destroy_tap_function();
    if (!destroy_tap || destroy_tap(session->tap) != noErr) result = -4;
    session->tap = kAudioObjectUnknown;
  }
  free(session);
  return result;
}

static void *hear_tap_start(void *context, HearAudioCallback callback, int *stage) {
  if (stage) *stage = 0;
  if (!context || !callback) {
    if (stage) *stage = 20;
    return NULL;
  }
  // Failure to relax an idle-sleep assertion must not interrupt recording.
  // The capture owner reports the policy result in unified diagnostics.
  (void)hear_allow_audio_idle_sleep();
  HearTapSession *session = calloc(1, sizeof(*session));
  if (!session) {
    if (stage) *stage = 20;
    return NULL;
  }
  session->tap = kAudioObjectUnknown;
  session->device = kAudioObjectUnknown;
  session->callback = callback;
  session->callback_context = context;
  atomic_init(&session->format_error, 0);
  atomic_init(&session->last_frame_ns, monotonic_ns());

  @autoreleasepool {
    NSDictionary *configuration = nil;
    CATapDescription *description = nil;
    HearCreateTap create_tap = hear_create_tap_function();
    HearDestroyTap destroy_tap = hear_destroy_tap_function();
    if (!create_tap || !destroy_tap) {
      if (stage) *stage = 20;
      goto fail;
    }
    description = [[NSClassFromString(@"CATapDescription") alloc]
        initMonoGlobalTapButExcludeProcesses:@[]];
    if (!description) {
      if (stage) *stage = 20;
      goto fail;
    }
    description.name = @"hear system audio";
    description.private = YES;
    description.muteBehavior = CATapUnmuted;
    NSOperatingSystemVersion version = [[NSProcessInfo processInfo] operatingSystemVersion];
    if (version.majorVersion >= 26) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
      // Bundle exclusions survive GUI launches after the login item starts.
      description.bundleIDs = @[@"app.hear.desktop", @"app.hear.core"];
      description.processRestoreEnabled = YES;
#pragma clang diagnostic pop
    }
    if (create_tap(description, &session->tap) != noErr) {
      if (stage) *stage = 22;
      goto fail;
    }

    AudioObjectPropertyAddress address = {
      kAudioTapPropertyUID, kAudioObjectPropertyScopeGlobal,
      kAudioObjectPropertyElementMain
    };
    CFStringRef tap_uid = NULL;
    UInt32 size = sizeof(tap_uid);
    OSStatus status = AudioObjectGetPropertyData(session->tap, &address, 0, NULL,
                                                 &size, &tap_uid);
    if (status != noErr || !tap_uid) {
      if (stage) *stage = 23;
      goto fail;
    }
    configuration = @{
      @(kAudioAggregateDeviceNameKey): @"hear system audio input",
      @(kAudioAggregateDeviceUIDKey): [[NSUUID UUID] UUIDString],
      @(kAudioAggregateDeviceIsPrivateKey): @YES,
      @(kAudioAggregateDeviceTapListKey): @[@{
        @(kAudioSubTapUIDKey): (__bridge NSString *)tap_uid
      }],
    };
    status = AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)configuration,
                                                 &session->device);
    CFRelease(tap_uid);
    if (status != noErr) {
      if (stage) *stage = 24;
      goto fail;
    }

    address.mSelector = kAudioDevicePropertyStreamFormat;
    address.mScope = kAudioDevicePropertyScopeInput;
    AudioStreamBasicDescription format = {0};
    size = sizeof(format);
    status = AudioObjectGetPropertyData(session->device, &address, 0, NULL,
                                        &size, &format);
    if (status != noErr || format.mFormatID != kAudioFormatLinearPCM ||
        !(format.mFormatFlags & kAudioFormatFlagIsFloat) ||
        format.mBitsPerChannel != 32 || format.mChannelsPerFrame != 1 ||
        format.mSampleRate < 8000 || format.mSampleRate > 192000) {
      if (stage) *stage = 25;
      goto fail;
    }
    session->sample_rate = (uint32_t)format.mSampleRate;
    status = AudioDeviceCreateIOProcID(session->device, hear_tap_receive,
                                       session, &session->io_proc);
    if (status != noErr) {
      if (stage) *stage = 26;
      goto fail;
    }
    status = AudioDeviceStart(session->device, session->io_proc);
    if (status != noErr) {
      if (stage) *stage = 27;
      goto fail;
    }
    session->started = true;
    atomic_store(&session->last_frame_ns, monotonic_ns());
    return session;
  fail:
    if (hear_tap_stop(session) == -2) {
      // A partially created IOProc may still reference its context.
      _exit(70);
    }
    return NULL;
  }
}

static int hear_tap_error(void *handle) {
  HearTapSession *session = handle;
  if (!session) return -1;
  if (atomic_load(&session->format_error)) return 1;
  uint64_t elapsed = monotonic_ns() - atomic_load(&session->last_frame_ns);
  if (elapsed <= 10000000000ULL) return 0;
  AudioObjectPropertyAddress address = {
    kAudioDevicePropertyDeviceIsAlive, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyElementMain
  };
  UInt32 alive = 0;
  UInt32 size = sizeof(alive);
  if (AudioObjectGetPropertyData(session->device, &address, 0, NULL,
                                 &size, &alive) != noErr || !alive) return 3;
  return 0;
}

int hear_system_audio_tap_available(void) {
  NSOperatingSystemVersion version = [[NSProcessInfo processInfo] operatingSystemVersion];
  // Earlier Tap releases only accept currently connected audio process IDs.
  // They cannot exclude a GUI that launches after the login item. macOS 26
  // adds persistent bundle-ID exclusions, which protect hear's own playback.
  return version.majorVersion >= 26;
}

void *hear_system_audio_start(void *context, HearAudioCallback callback, int *stage,
                              int prefer_tap) {
  HearSystemSession *outer = calloc(1, sizeof(*outer));
  if (!outer) {
    if (stage) *stage = 20;
    return NULL;
  }
  outer->uses_tap = prefer_tap && hear_system_audio_tap_available();
  outer->session = outer->uses_tap ? hear_tap_start(context, callback, stage)
                                   : hear_legacy_system_audio_start(context, callback, stage);
  if (!outer->session) {
    free(outer);
    return NULL;
  }
  return outer;
}

int hear_system_audio_error(void *handle) {
  HearSystemSession *outer = handle;
  if (!outer) return -1;
  return outer->uses_tap ? hear_tap_error(outer->session)
                         : hear_legacy_system_audio_error(outer->session);
}

int hear_system_audio_stop(void *handle) {
  HearSystemSession *outer = handle;
  if (!outer) return -1;
  int status = outer->uses_tap ? hear_tap_stop(outer->session)
                                : hear_legacy_system_audio_stop(outer->session);
  if (status != -2) free(outer);
  return status;
}

typedef struct {
  atomic_uint_fast64_t frames;
  atomic_uint_fast64_t peak_millionths;
} HearTapProbe;

static void hear_probe_receive(void *context, const float *samples, uint32_t frames,
                               uint32_t rate, uint32_t channels) {
  (void)rate;
  HearTapProbe *probe = context;
  atomic_fetch_add(&probe->frames, frames);
  uint64_t peak = atomic_load(&probe->peak_millionths);
  for (size_t i = 0; i < (size_t)frames * channels; ++i) {
    uint64_t value = (uint64_t)(fminf(fabsf(samples[i]), 1.0f) * 1000000.0f);
    if (value > peak) peak = value;
  }
  uint64_t previous = atomic_load(&probe->peak_millionths);
  while (peak > previous &&
         !atomic_compare_exchange_weak(&probe->peak_millionths, &previous, peak)) {}
}

int64_t hear_system_audio_probe(uint32_t seconds, uint64_t *peak_millionths_out) {
  if (!hear_system_audio_tap_available()) {
    return hear_legacy_system_audio_probe(seconds, peak_millionths_out);
  }
  HearTapProbe *probe = calloc(1, sizeof(*probe));
  if (!probe) return -20;
  atomic_init(&probe->frames, 0);
  atomic_init(&probe->peak_millionths, 0);
  int stage = 0;
  void *session = hear_tap_start(probe, hear_probe_receive, &stage);
  if (!session) {
    free(probe);
    return -stage;
  }
  sleep(seconds);
  int error = hear_tap_error(session);
  int64_t frames = (int64_t)atomic_load(&probe->frames);
  uint64_t peak = atomic_load(&probe->peak_millionths);
  int stop_status = hear_tap_stop(session);
  if (stop_status == -2) return -28; // Keep the probe callback context alive.
  free(probe);
  if (error) return -29;
  if (stop_status) return -30;
  if (peak_millionths_out) *peak_millionths_out = peak;
  return frames;
}
