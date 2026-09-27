#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreAudio/CoreAudioTypes.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdatomic.h>
#include <stdint.h>
#include <math.h>

@interface HearSystemOutput : NSObject <SCStreamOutput, SCStreamDelegate> {
@public
  atomic_uint_fast64_t frames;
  atomic_uint_fast64_t peak_millionths;
  atomic_int stopped_with_error;
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
    }
    if (backing) CFRelease(backing);
  }
}

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
  (void)stream;
  atomic_store(&stopped_with_error, (int)error.code ?: -1);
}
@end

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
