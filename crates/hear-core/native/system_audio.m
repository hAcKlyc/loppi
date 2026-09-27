#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreAudio/CoreAudioTypes.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdatomic.h>
#include <stdint.h>
#include <math.h>

typedef void (*HearAudioCallback)(void *context, const float *samples,
                                  uint32_t frames, uint32_t rate, uint32_t channels);

@interface HearSystemOutput : NSObject <SCStreamOutput, SCStreamDelegate> {
@public
  atomic_uint_fast64_t frames;
  atomic_uint_fast64_t peak_millionths;
  atomic_int stopped_with_error;
  HearAudioCallback callback;
  void *callback_context;
}
@end

@implementation HearSystemOutput
- (instancetype)init {
  self = [super init];
  if (self) {
    atomic_init(&frames, 0);
    atomic_init(&peak_millionths, 0);
    atomic_init(&stopped_with_error, 0);
  }
  return self;
}

- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)buffer
        ofType:(SCStreamOutputType)type {
  (void)stream;
  if (type == SCStreamOutputTypeAudio && CMSampleBufferIsValid(buffer)) {
    atomic_fetch_add(&frames, CMSampleBufferGetNumSamples(buffer));
    CMAudioFormatDescriptionRef description = (CMAudioFormatDescriptionRef)CMSampleBufferGetFormatDescription(buffer);
    const AudioStreamBasicDescription *format = CMAudioFormatDescriptionGetStreamBasicDescription(description);
    if (!format || format->mFormatID != kAudioFormatLinearPCM ||
        format->mBitsPerChannel != 32 || !(format->mFormatFlags & kAudioFormatFlagIsFloat)) {
      return;
    }
    AudioBufferList list;
    CMBlockBufferRef backing = NULL;
    OSStatus status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        buffer, NULL, &list, sizeof(list), kCFAllocatorDefault, kCFAllocatorDefault,
        kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, &backing);
    if (status == noErr && list.mNumberBuffers == 1 && list.mBuffers[0].mData) {
      const float *samples = (const float *)list.mBuffers[0].mData;
      size_t count = list.mBuffers[0].mDataByteSize / sizeof(float);
      uint64_t current = atomic_load(&peak_millionths);
      for (size_t index = 0; index < count; index++) {
        uint64_t value = (uint64_t)(fminf(fabsf(samples[index]), 1.0f) * 1000000.0f);
        if (value > current) current = value;
      }
      uint64_t old = atomic_load(&peak_millionths);
      while (current > old && !atomic_compare_exchange_weak(&peak_millionths, &old, current)) {}
      if (callback && callback_context) {
        callback(callback_context, samples,
                 (uint32_t)CMSampleBufferGetNumSamples(buffer),
                 (uint32_t)format->mSampleRate,
                 (uint32_t)format->mChannelsPerFrame);
      }
    }
    if (backing) CFRelease(backing);
  }
}

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
  (void)stream;
  atomic_store(&stopped_with_error, (int)error.code ?: -1);
}
@end

@interface HearSystemSession : NSObject
@property (strong) SCStream *stream;
@property (strong) HearSystemOutput *output;
@property (strong) dispatch_queue_t queue;
@end
@implementation HearSystemSession
@end

static void hear_disable_callback(HearSystemSession *session) {
  // All SCStreamOutput callbacks use this serial queue. Clearing the Rust
  // pointer here waits for a callback already in flight to return.
  dispatch_sync(session.queue, ^{
    session.output->callback = NULL;
    session.output->callback_context = NULL;
  });
}

// The caller holds the returned session until stop completes. A queue barrier
// ensures no callback can use its Rust context after this function returns.
void *hear_system_audio_start(void *context, HearAudioCallback callback, int *stage) {
  if (stage) *stage = 0;
  if (!context || !callback || !CGPreflightScreenCaptureAccess()) {
    if (stage) *stage = 7;
    return NULL;
  }
  __block SCShareableContent *content = nil;
  __block NSError *discovery_error = nil;
  dispatch_semaphore_t discovery = dispatch_semaphore_create(0);
  [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                      onScreenWindowsOnly:NO
                                         completionHandler:^(SCShareableContent *found,
                                                             NSError *error) {
    content = found;
    discovery_error = error;
    dispatch_semaphore_signal(discovery);
  }];
  if (dispatch_semaphore_wait(discovery, dispatch_time(DISPATCH_TIME_NOW, 20 * NSEC_PER_SEC))) {
    if (stage) *stage = 1;
    return NULL;
  }
  if (discovery_error || content.displays.count == 0) {
    if (stage) *stage = 2;
    return NULL;
  }
  SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:content.displays.firstObject
                                                   excludingWindows:@[]];
  SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
  config.capturesAudio = YES;
  config.excludesCurrentProcessAudio = YES;
  config.sampleRate = 48000;
  config.channelCount = 1;
  config.width = 8;
  config.height = 8;
  config.minimumFrameInterval = CMTimeMake(1, 1);
  config.showsCursor = NO;

  HearSystemSession *session = [[HearSystemSession alloc] init];
  session.output = [[HearSystemOutput alloc] init];
  session.output->callback = callback;
  session.output->callback_context = context;
  session.queue = dispatch_queue_create("app.hear.system-audio.capture", DISPATCH_QUEUE_SERIAL);
  session.stream = [[SCStream alloc] initWithFilter:filter
                                      configuration:config
                                           delegate:session.output];
  NSError *add_error = nil;
  if (![session.stream addStreamOutput:session.output type:SCStreamOutputTypeAudio
                    sampleHandlerQueue:session.queue error:&add_error]) {
    if (stage) *stage = 3;
    return NULL;
  }
  dispatch_semaphore_t started = dispatch_semaphore_create(0);
  __block NSError *start_error = nil;
  [session.stream startCaptureWithCompletionHandler:^(NSError *error) {
    start_error = error;
    dispatch_semaphore_signal(started);
  }];
  if (dispatch_semaphore_wait(started, dispatch_time(DISPATCH_TIME_NOW, 20 * NSEC_PER_SEC))) {
    hear_disable_callback(session);
    [session.stream stopCaptureWithCompletionHandler:^(NSError *error) { (void)error; }];
    if (stage) *stage = 4;
    return NULL;
  }
  if (start_error) {
    hear_disable_callback(session);
    if (stage) *stage = 5;
    return NULL;
  }
  return (void *)CFBridgingRetain(session);
}

int hear_system_audio_error(void *handle) {
  if (!handle) return -1;
  HearSystemSession *session = (__bridge HearSystemSession *)handle;
  return atomic_load(&session.output->stopped_with_error);
}

int hear_system_audio_stop(void *handle) {
  if (!handle) return -1;
  HearSystemSession *session = CFBridgingRelease(handle);
  dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
  __block NSError *stop_error = nil;
  [session.stream stopCaptureWithCompletionHandler:^(NSError *error) {
    stop_error = error;
    dispatch_semaphore_signal(stopped);
  }];
  if (dispatch_semaphore_wait(stopped, dispatch_time(DISPATCH_TIME_NOW, 20 * NSEC_PER_SEC))) {
    // Keep the callback context alive on timeout: the caller must treat this
    // as a fatal source failure and terminate before releasing its context.
    (void)CFBridgingRetain(session);
    return -2;
  }
  hear_disable_callback(session);
  return stop_error ? (int)stop_error.code ?: -3 : 0;
}

// Returns captured audio frames, or a negative stage code. This short diagnostic
// uses the same signed helper identity as production capture and never saves audio.
int64_t hear_system_audio_probe(uint32_t seconds, uint64_t *peak_millionths_out) {
  if (!CGPreflightScreenCaptureAccess()) {
    return -7;
  }
  __block SCShareableContent *content = nil;
  __block int error_code = 0;
  dispatch_semaphore_t discovery = dispatch_semaphore_create(0);
  [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                      onScreenWindowsOnly:NO
                                         completionHandler:^(SCShareableContent *found,
                                                             NSError *error) {
    content = found;
    error_code = (int)error.code;
    dispatch_semaphore_signal(discovery);
  }];
  if (dispatch_semaphore_wait(discovery, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC))) {
    return -1;
  }
  if (error_code || content.displays.count == 0) {
    return -2;
  }

  SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:content.displays.firstObject
                                                   excludingWindows:@[]];
  SCStreamConfiguration *configuration = [[SCStreamConfiguration alloc] init];
  configuration.capturesAudio = YES;
  configuration.excludesCurrentProcessAudio = YES;
  configuration.sampleRate = 48000;
  configuration.channelCount = 1;
  configuration.width = 8;
  configuration.height = 8;
  configuration.minimumFrameInterval = CMTimeMake(1, 1);
  configuration.showsCursor = NO;

  HearSystemOutput *output = [[HearSystemOutput alloc] init];
  SCStream *stream = [[SCStream alloc] initWithFilter:filter
                                       configuration:configuration
                                            delegate:output];
  NSError *add_error = nil;
  if (![stream addStreamOutput:output type:SCStreamOutputTypeAudio
           sampleHandlerQueue:dispatch_queue_create("app.hear.system-audio", DISPATCH_QUEUE_SERIAL)
                        error:&add_error]) {
    return -3;
  }

  dispatch_semaphore_t started = dispatch_semaphore_create(0);
  __block int start_error = 0;
  [stream startCaptureWithCompletionHandler:^(NSError *error) {
    start_error = (int)error.code;
    dispatch_semaphore_signal(started);
  }];
  if (dispatch_semaphore_wait(started, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC))) {
    return -4;
  }
  if (start_error) {
    return -5;
  }
  [NSThread sleepForTimeInterval:seconds];
  dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
  __block int stop_error = 0;
  [stream stopCaptureWithCompletionHandler:^(NSError *error) {
    stop_error = (int)error.code;
    dispatch_semaphore_signal(stopped);
  }];
  if (dispatch_semaphore_wait(stopped, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC))) {
    return -8;
  }
  if (stop_error) {
    return -9;
  }
  if (atomic_load(&output->stopped_with_error)) {
    return -6;
  }
  if (peak_millionths_out) {
    *peak_millionths_out = atomic_load(&output->peak_millionths);
  }
  return (int64_t)atomic_load(&output->frames);
}
