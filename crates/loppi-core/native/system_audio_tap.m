#import <Foundation/Foundation.h>
#import <CoreAudio/AudioHardware.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#include <math.h>
#include <mach/mach_time.h>
#include <dlfcn.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>
#import "audio_exclusions.h"

// Resolve the 14.2 functions only on a supported OS. A direct Mach-O import
// would make the macOS 13 ScreenCaptureKit fallback fail before main runs.
typedef OSStatus (*LoppiCreateTap)(CATapDescription *, AudioObjectID *);
typedef OSStatus (*LoppiDestroyTap)(AudioObjectID);

static LoppiCreateTap loppi_create_tap_function(void) {
  return (LoppiCreateTap)dlsym(RTLD_DEFAULT, "AudioHardwareCreateProcessTap");
}

static LoppiDestroyTap loppi_destroy_tap_function(void) {
  return (LoppiDestroyTap)dlsym(RTLD_DEFAULT, "AudioHardwareDestroyProcessTap");
}

typedef void (*LoppiAudioCallback)(void *, const float *, uint32_t, uint32_t, uint32_t, uint64_t);

extern int loppi_allow_audio_idle_sleep(void);
extern void *loppi_legacy_system_audio_start(void *, LoppiAudioCallback, int *, int *);
extern int loppi_legacy_system_audio_error(void *);
extern int loppi_legacy_system_audio_stop(void *);
extern int64_t loppi_legacy_system_audio_probe(uint32_t, uint64_t *);
extern uint64_t loppi_legacy_system_audio_exclusion_generation(void *);

static NSArray<NSNumber *> *loppi_tap_exclusions(uint64_t *identity, uint64_t *playback) {
  AudioObjectPropertyAddress address={kAudioHardwarePropertyProcessObjectList,kAudioObjectPropertyScopeGlobal,kAudioObjectPropertyElementMain};
  UInt32 size=0;
  if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject,&address,0,NULL,&size)!=noErr || size>65536) return nil;
  AudioObjectID *ids=calloc(1,MAX(size,1));
  if (!ids) return nil;
  if (AudioObjectGetPropertyData(kAudioObjectSystemObject,&address,0,NULL,&size,ids)!=noErr) { free(ids); return nil; }
  NSArray<NSNumber *> *gui=loppi_gui_pids();
  NSMutableSet<NSNumber *> *found=[NSMutableSet set];
  NSMutableArray<NSNumber *> *excluded=[NSMutableArray array];
  for (UInt32 i=0;i<size/sizeof(AudioObjectID);i++) {
    pid_t pid=0; UInt32 bytes=sizeof(pid); address.mSelector=kAudioProcessPropertyPID;
    if (AudioObjectGetPropertyData(ids[i],&address,0,NULL,&bytes,&pid)!=noErr) continue;
    BOOL isGUI=[gui containsObject:@(pid)];
    CFStringRef bundle=NULL; bytes=sizeof(bundle); address.mSelector=kAudioProcessPropertyBundleID;
    AudioObjectGetPropertyData(ids[i],&address,0,NULL,&bytes,&bundle);
    NSString *name=(__bridge NSString *)bundle;
    if (isGUI || [name isEqual:@"app.loppi.desktop"] || [name isEqual:@"app.loppi.core"]) [excluded addObject:@(ids[i])];
    if (isGUI) [found addObject:@(pid)];
    if (bundle) CFRelease(bundle);
  }
  free(ids);
  [excluded sortUsingSelector:@selector(compare:)];
  *identity=loppi_exclusion_hash(excluded);
  *playback=found.count==gui.count ? loppi_exclusion_hash(gui) : 0;
  return excluded;
}

typedef struct {
  AudioObjectID tap;
  AudioObjectID device;
  AudioDeviceIOProcID io_proc;
  uint32_t sample_rate;
  mach_timebase_info_data_t timebase;
  LoppiAudioCallback callback;
  void *callback_context;
  atomic_uint_fast64_t last_frame_ns;
  atomic_int format_error;
  bool started;
  CFTypeRef description;
  uint64_t exclusion_identity;
  uint64_t playback_generation;
  uint64_t exclusion_checked_ns;
} LoppiTapSession;

typedef struct {
  bool uses_tap;
  void *session;
} LoppiSystemSession;

static uint64_t monotonic_ns(void) {
  struct timespec now;
  clock_gettime(CLOCK_MONOTONIC, &now);
  return (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec;
}

static OSStatus loppi_tap_receive(AudioObjectID device, const AudioTimeStamp *now,
                                  const AudioBufferList *input, const AudioTimeStamp *input_time,
                                  AudioBufferList *output, const AudioTimeStamp *output_time,
                                  void *context) {
  (void)device;
  (void)now;
  (void)output;
  (void)output_time;
  LoppiTapSession *session = context;
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
                      session->sample_rate, 1,
                      input_time && (input_time->mFlags & kAudioTimeStampHostTimeValid)
                        ? (uint64_t)(((__uint128_t)input_time->mHostTime * session->timebase.numer)
                                     / session->timebase.denom) : 0);
  }
  return noErr;
}

// A successful AudioDeviceStop drains the IOProc before its Rust callback
// context is released. If HAL cannot stop/destroy it, keep the session alive
// and let the capture owner terminate instead of risking a callback into freed
// memory.
static int loppi_tap_stop(LoppiTapSession *session) {
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
    LoppiDestroyTap destroy_tap = loppi_destroy_tap_function();
    if (!destroy_tap || destroy_tap(session->tap) != noErr) result = -4;
    session->tap = kAudioObjectUnknown;
  }
  if (session->description) CFRelease(session->description);
  free(session);
  return result;
}

static void *loppi_tap_start(void *context, LoppiAudioCallback callback, int *stage) {
  if (stage) *stage = 0;
  if (!context || !callback) {
    if (stage) *stage = 20;
    return NULL;
  }
  // Failure to relax an idle-sleep assertion must not interrupt recording.
  // The capture owner reports the policy result in unified diagnostics.
  (void)loppi_allow_audio_idle_sleep();
  LoppiTapSession *session = calloc(1, sizeof(*session));
  if (!session) {
    if (stage) *stage = 20;
    return NULL;
  }
  mach_timebase_info(&session->timebase);
  if (session->timebase.denom == 0) { free(session); return NULL; }
  session->tap = kAudioObjectUnknown;
  session->device = kAudioObjectUnknown;
  session->callback = callback;
  session->callback_context = context;
  atomic_init(&session->format_error, 0);
  atomic_init(&session->last_frame_ns, monotonic_ns());

  @autoreleasepool {
    NSDictionary *configuration = nil;
    CATapDescription *description = nil;
    NSArray<NSNumber *> *exclusions = nil;
    LoppiCreateTap create_tap = loppi_create_tap_function();
    LoppiDestroyTap destroy_tap = loppi_destroy_tap_function();
    if (!create_tap || !destroy_tap) {
      if (stage) *stage = 20;
      goto fail;
    }
    exclusions=loppi_tap_exclusions(&session->exclusion_identity,&session->playback_generation);
    if (!exclusions) { if (stage) *stage=21; goto fail; }
    description = [[NSClassFromString(@"CATapDescription") alloc]
        initMonoGlobalTapButExcludeProcesses:exclusions];
    if (!description) {
      if (stage) *stage = 20;
      goto fail;
    }
    description.name = @"loppi system audio";
    description.private = YES;
    description.muteBehavior = CATapUnmuted;
    session->description=CFRetain((__bridge CFTypeRef)description);
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
      @(kAudioAggregateDeviceNameKey): @"loppi system audio input",
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
    status = AudioDeviceCreateIOProcID(session->device, loppi_tap_receive,
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
    if (loppi_tap_stop(session) == -2) {
      // A partially created IOProc may still reference its context.
      _exit(70);
    }
    return NULL;
  }
}

static int loppi_tap_error(void *handle) {
  LoppiTapSession *session = handle;
  if (!session) return -1;
  if (atomic_load(&session->format_error)) return 1;
  uint64_t now=monotonic_ns();
  if (now-session->exclusion_checked_ns>=250000000ULL) {
    session->exclusion_checked_ns=now;
    @autoreleasepool {
      uint64_t identity=0,playback=0;
      NSArray<NSNumber *> *excluded=loppi_tap_exclusions(&identity,&playback);
      if (!excluded) return 4;
      if (identity!=session->exclusion_identity) {
        CATapDescription *description=(__bridge CATapDescription *)session->description;
        description.processes=excluded;
        AudioObjectPropertyAddress address={kAudioTapPropertyDescription,kAudioObjectPropertyScopeGlobal,kAudioObjectPropertyElementMain};
        if (AudioObjectSetPropertyData(session->tap,&address,0,NULL,sizeof(description),&description)!=noErr) return 4;
        session->exclusion_identity=identity;
      }
      session->playback_generation=playback;
    }
  }
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

int loppi_system_audio_tap_available(void) {
  NSOperatingSystemVersion version = [[NSProcessInfo processInfo] operatingSystemVersion];
  return (version.majorVersion>14 || (version.majorVersion==14 && version.minorVersion>=2))
      && loppi_create_tap_function() && loppi_destroy_tap_function();
}

uint64_t loppi_playback_exclusion_generation(void) { @autoreleasepool { return loppi_gui_generation(); } }
uint64_t loppi_system_audio_exclusion_generation(void *handle) {
  LoppiSystemSession *outer=handle;
  if (!outer) return 0;
  return outer->uses_tap ? ((LoppiTapSession *)outer->session)->playback_generation
      : loppi_legacy_system_audio_exclusion_generation(outer->session);
}

void *loppi_system_audio_start(void *context, LoppiAudioCallback callback, int *stage,
                              int prefer_tap, int *native_error) {
  if (native_error) *native_error = 0;
  LoppiSystemSession *outer = calloc(1, sizeof(*outer));
  if (!outer) {
    if (stage) *stage = 20;
    return NULL;
  }
  outer->uses_tap = prefer_tap && loppi_system_audio_tap_available();
  outer->session = outer->uses_tap ? loppi_tap_start(context, callback, stage)
                                   : loppi_legacy_system_audio_start(context, callback, stage, native_error);
  if (!outer->session) {
    free(outer);
    return NULL;
  }
  return outer;
}

int loppi_system_audio_error(void *handle) {
  LoppiSystemSession *outer = handle;
  if (!outer) return -1;
  return outer->uses_tap ? loppi_tap_error(outer->session)
                         : loppi_legacy_system_audio_error(outer->session);
}

int loppi_system_audio_stop(void *handle) {
  LoppiSystemSession *outer = handle;
  if (!outer) return -1;
  int status = outer->uses_tap ? loppi_tap_stop(outer->session)
                                : loppi_legacy_system_audio_stop(outer->session);
  if (status != -2) free(outer);
  return status;
}

typedef struct {
  atomic_uint_fast64_t frames;
  atomic_uint_fast64_t peak_millionths;
} LoppiTapProbe;

static void loppi_probe_receive(void *context, const float *samples, uint32_t frames,
                               uint32_t rate, uint32_t channels, uint64_t sample_start_ns) {
  (void)rate;
  (void)sample_start_ns;
  LoppiTapProbe *probe = context;
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

int64_t loppi_system_audio_probe(uint32_t seconds, uint64_t *peak_millionths_out) {
  if (!loppi_system_audio_tap_available()) {
    return loppi_legacy_system_audio_probe(seconds, peak_millionths_out);
  }
  LoppiTapProbe *probe = calloc(1, sizeof(*probe));
  if (!probe) return -20;
  atomic_init(&probe->frames, 0);
  atomic_init(&probe->peak_millionths, 0);
  int stage = 0;
  void *session = loppi_tap_start(probe, loppi_probe_receive, &stage);
  if (!session) {
    free(probe);
    return -stage;
  }
  sleep(seconds);
  int error = loppi_tap_error(session);
  int64_t frames = (int64_t)atomic_load(&probe->frames);
  uint64_t peak = atomic_load(&probe->peak_millionths);
  int stop_status = loppi_tap_stop(session);
  if (stop_status == -2) return -28; // Keep the probe callback context alive.
  free(probe);
  if (error) return -29;
  if (stop_status) return -30;
  if (peak_millionths_out) *peak_millionths_out = peak;
  return frames;
}
