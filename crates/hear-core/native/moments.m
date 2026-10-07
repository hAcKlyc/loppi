#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <ImageIO/ImageIO.h>
#import <Vision/Vision.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>

int hear_active_display_rect(int32_t *x, int32_t *y, int32_t *width,
                             int32_t *height) {
  if (!x || !y || !width || !height) return 0;
  CGEventRef event = CGEventCreate(NULL);
  if (!event) return 0;
  CGPoint location = CGEventGetLocation(event);
  CFRelease(event);
  CGDirectDisplayID display = 0;
  uint32_t count = 0;
  if (CGGetDisplaysWithPoint(location, 1, &display, &count) != kCGErrorSuccess ||
      count == 0) {
    display = CGMainDisplayID();
  }
  CGRect bounds = CGDisplayBounds(display);
  if (bounds.size.width <= 0 || bounds.size.height <= 0) return 0;
  *x = (int32_t)floor(bounds.origin.x);
  *y = (int32_t)floor(bounds.origin.y);
  *width = (int32_t)ceil(bounds.size.width);
  *height = (int32_t)ceil(bounds.size.height);
  return 1;
}

int hear_ocr_png(const char *path, char **output) {
  if (!path || !output) return 0;
  *output = NULL;
  @autoreleasepool {
    NSString *name = [NSString stringWithUTF8String:path];
    if (!name) return 0;
    NSURL *url = [NSURL fileURLWithPath:name];
    CGImageSourceRef source = CGImageSourceCreateWithURL((__bridge CFURLRef)url, NULL);
    if (!source) return 0;
    CGImageRef image = CGImageSourceCreateImageAtIndex(source, 0, NULL);
    CFRelease(source);
    if (!image) return 0;
    VNRecognizeTextRequest *request = [[VNRecognizeTextRequest alloc] init];
    request.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
    request.recognitionLanguages = @[ @"zh-Hans", @"en-US" ];
    request.usesLanguageCorrection = YES;
    VNImageRequestHandler *handler =
        [[VNImageRequestHandler alloc] initWithCGImage:image options:@{}];
    NSError *error = nil;
    BOOL completed = [handler performRequests:@[ request ] error:&error];
    CGImageRelease(image);
    if (!completed || error) return 0;
    NSMutableArray<NSString *> *lines = [NSMutableArray array];
    for (VNRecognizedTextObservation *observation in request.results) {
      VNRecognizedText *candidate = [[observation topCandidates:1] firstObject];
      if (candidate.string.length > 0) [lines addObject:candidate.string];
    }
    NSString *joined = [lines componentsJoinedByString:@"\n"];
    *output = strdup(joined.UTF8String ?: "");
    return *output ? 1 : 0;
  }
}

#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreImage/CoreImage.h>
#import <CoreMedia/CoreMedia.h>

@interface HearShot : NSObject <SCStreamOutput>
@property NSLock *lock;
@property dispatch_semaphore_t signal;
@property BOOL finished;
@property CGImageRef image;
@property SCStream *stream;
@property dispatch_semaphore_t startSignal;
@property dispatch_semaphore_t stopSignal;
@property BOOL started;
@property BOOL terminationRequested;
@property BOOL stopRequested;
@property BOOL stopCompleted;
@property BOOL stopFailed;
@end
@implementation HearShot
- (instancetype)init { if ((self=[super init])) { _lock=[NSLock new]; _signal=dispatch_semaphore_create(0); _startSignal=dispatch_semaphore_create(0); _stopSignal=dispatch_semaphore_create(0); } return self; }
- (void)dealloc { if (_image) CGImageRelease(_image); }
- (void)finish:(CGImageRef)image {
  [_lock lock];
  if (!_finished) { _finished=YES; if(image) _image=CGImageRetain(image); dispatch_semaphore_signal(_signal); }
  [_lock unlock];
}
- (void)stopStream {
  [_stream stopCaptureWithCompletionHandler:^(NSError *error) {
    [self.lock lock]; self.stopCompleted=YES; self.stopFailed=error!=nil; self.stream=nil; [self.lock unlock];
    dispatch_semaphore_signal(self.stopSignal);
  }];
}
- (void)terminate {
  [_lock lock];
  _terminationRequested=YES;
  BOOL stop=_started && !_stopRequested;
  if(stop) _stopRequested=YES;
  [_lock unlock];
  if(stop) [self stopStream];
}
- (void)startFinished:(NSError *)error {
  [_lock lock]; _started=error==nil;
  if(error) _stream=nil;
  BOOL stop=_terminationRequested && _started && !_stopRequested;
  if(stop) _stopRequested=YES;
  [_lock unlock];
  if(error) [self finish:NULL];
  dispatch_semaphore_signal(_startSignal);
  if(stop) [self stopStream];
}
- (BOOL)stoppedSuccessfully {
  [_lock lock]; BOOL success=_stopCompleted && !_stopFailed; [_lock unlock]; return success;
}
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)buffer ofType:(SCStreamOutputType)type {
  if(type!=SCStreamOutputTypeScreen || !CMSampleBufferIsValid(buffer)) return;
  [_lock lock]; BOOL done=_finished; [_lock unlock];
  if(done) return;
  NSArray *attachments=(__bridge NSArray *)CMSampleBufferGetSampleAttachmentsArray(buffer, NO);
  NSNumber *status=attachments.firstObject[SCStreamFrameInfoStatus];
  if(!status || status.integerValue!=SCFrameStatusComplete) return;
  CVPixelBufferRef pixel=CMSampleBufferGetImageBuffer(buffer);
  if(!pixel) return;
  CIImage *ci=[CIImage imageWithCVPixelBuffer:pixel];
  CGImageRef image=[[CIContext contextWithOptions:nil] createCGImage:ci fromRect:ci.extent];
  [self finish:image]; if(image) CGImageRelease(image);
}
@end

// Called off the main thread. No late callback ever writes a file: after the bounded
// wait only this caller can encode the retained image into its private pending path.
int hear_capture_png(const char *path, const uint32_t *ids, size_t count, int32_t peerPID) {
  if(!path || count>8 || peerPID<=0 || !CGPreflightScreenCaptureAccess()) return 0;
  @autoreleasepool {
    dispatch_semaphore_t ready=dispatch_semaphore_create(0);
    NSLock *lock=[NSLock new];
    __block SCShareableContent *content=nil;
    __block BOOL abandoned=NO;
    [SCShareableContent getShareableContentExcludingDesktopWindows:NO onScreenWindowsOnly:NO completionHandler:^(SCShareableContent *value, NSError *error) {
      [lock lock]; if(!abandoned && !error) content=value; [lock unlock]; dispatch_semaphore_signal(ready);
    }];
    if(dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, 3*NSEC_PER_SEC))) {
      [lock lock]; abandoned=YES; [lock unlock]; return -3;
    }
    if(!content) return 0;
    NSMutableArray<SCWindow *> *excluded=[NSMutableArray array];
    NSArray *windows=CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionAll,kCGNullWindowID));
    for(size_t i=0;i<count;i++) {
      SCWindow *found=nil;
      for(SCWindow *w in content.windows) if(w.windowID==ids[i]) { found=w; break; }
      if(found) {
        if(found.owningApplication.processID!=peerPID) return -2;
        [excluded addObject:found];
      } else {
        for(NSDictionary *info in windows) if([info[(__bridge NSString *)kCGWindowNumber] unsignedIntValue]==ids[i]) {
          if([info[(__bridge NSString *)kCGWindowOwnerPID] intValue]!=peerPID ||
             [info[(__bridge NSString *)kCGWindowIsOnscreen] boolValue]) return -2;
        }
      }
    }
    CGEventRef event=CGEventCreate(NULL);
    CGPoint point=event ? CGEventGetLocation(event) : CGPointZero;
    if(event) CFRelease(event);
    CGDirectDisplayID displayID=CGMainDisplayID(); uint32_t n=0;
    CGDirectDisplayID underMouse=0;
    if(CGGetDisplaysWithPoint(point,1,&underMouse,&n)==kCGErrorSuccess && n) displayID=underMouse;
    SCDisplay *display=nil;
    for(SCDisplay *d in content.displays) if(d.displayID==displayID) { display=d; break; }
    if(!display) return 0;
    SCContentFilter *filter=[[SCContentFilter alloc] initWithDisplay:display excludingWindows:excluded];
    SCStreamConfiguration *config=[SCStreamConfiguration new];
    CGDisplayModeRef mode=CGDisplayCopyDisplayMode(displayID);
    config.width=mode ? CGDisplayModeGetPixelWidth(mode) : display.width;
    config.height=mode ? CGDisplayModeGetPixelHeight(mode) : display.height;
    if(mode) CGDisplayModeRelease(mode);
    config.showsCursor=NO; config.capturesAudio=NO;
    config.pixelFormat=kCVPixelFormatType_32BGRA;
    config.minimumFrameInterval=CMTimeMake(1,30); config.queueDepth=3;
    HearShot *shot=[HearShot new];
    SCStream *stream=nil;
    // Match the existing native bridges: a runtime OS check avoids compiler
    // availability helper symbols that the Rust linker does not supply.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
    if(NSProcessInfo.processInfo.operatingSystemVersion.majorVersion >= 14) {
      [SCScreenshotManager captureImageWithFilter:filter configuration:config completionHandler:^(CGImageRef image,NSError *error) { [shot finish:error ? NULL : image]; }];
    } else {
      stream=[[SCStream alloc] initWithFilter:filter configuration:config delegate:nil];
      shot.stream=stream;
      NSError *error=nil;
      if(![stream addStreamOutput:shot type:SCStreamOutputTypeScreen sampleHandlerQueue:dispatch_queue_create("hear.screenshot",DISPATCH_QUEUE_SERIAL) error:&error]) { shot.stream=nil; return 0; }
      [stream startCaptureWithCompletionHandler:^(NSError *error) { [shot startFinished:error]; }];
    }

#pragma clang diagnostic pop
    dispatch_time_t deadline=dispatch_time(DISPATCH_TIME_NOW,5*NSEC_PER_SEC);
    BOOL timeout=dispatch_semaphore_wait(shot.signal,deadline)!=0;
    if(timeout) [shot finish:NULL];
    if(stream) {
      // A timed-out start still owns its late completion, which must stop once
      // startup succeeds. Never issue an early stop and assume it was effective.
      if(!timeout && dispatch_semaphore_wait(shot.startSignal,deadline)) timeout=YES;
      if(timeout) { [shot finish:NULL]; [shot terminate]; return -3; }
      [shot terminate];
      if(shot.started) {
        if(dispatch_semaphore_wait(shot.stopSignal,dispatch_time(DISPATCH_TIME_NOW,3*NSEC_PER_SEC))) return -3;
        if(![shot stoppedSuccessfully]) return 0;
      }
    }
    if(timeout) return -3;
    if(!shot.image) return 0;
    NSURL *url=[NSURL fileURLWithPath:[NSString stringWithUTF8String:path]];
    CGImageDestinationRef destination=CGImageDestinationCreateWithURL((__bridge CFURLRef)url,CFSTR("public.png"),1,NULL);
    if(!destination) return 0;
    CGImageDestinationAddImage(destination,shot.image,NULL);
    BOOL saved=CGImageDestinationFinalize(destination); CFRelease(destination);
    return saved ? 1 : 0;
  }
}
