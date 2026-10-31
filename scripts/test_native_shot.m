// The production terminal owner is tested with delayed/failed native callbacks.
// This never captures a real screen or writes an image.
#import "../crates/loppi-core/native/moments.m"
#include <assert.h>
@interface FakeShotStream : NSObject
@property NSInteger stops;
@property NSError *stopError;
@property (copy) void (^completion)(NSError *);
@property BOOL delayedStop;
@end
@implementation FakeShotStream
- (void)stopCaptureWithCompletionHandler:(void (^)(NSError *))completion {
  _stops++;
  if(_delayedStop) _completion=[completion copy]; else completion(_stopError);
}
@end
static LoppiShot *fixture(FakeShotStream *stream) {
  LoppiShot *shot=[LoppiShot new]; shot.stream=(SCStream *)stream; return shot;
}
int main(void) { @autoreleasepool {
  FakeShotStream *late=[FakeShotStream new]; LoppiShot *timed=fixture(late);
  [timed finish:NULL]; [timed terminate];
  assert(late.stops==0); // Stop cannot run before successful startup.
  [timed startFinished:nil]; [timed terminate];
  assert(late.stops==1 && [timed stoppedSuccessfully]);
  assert(timed.image==NULL); // Late completion cannot establish a saved image.

  FakeShotStream *normal=[FakeShotStream new]; normal.delayedStop=YES;
  LoppiShot *shot=fixture(normal); [shot startFinished:nil]; [shot terminate];
  assert(normal.stops==1 && ![shot stoppedSuccessfully]);
  normal.completion(nil); normal.completion=nil;
  assert([shot stoppedSuccessfully]);

  FakeShotStream *failed=[FakeShotStream new];
  failed.stopError=[NSError errorWithDomain:@"test" code:1 userInfo:nil];
  LoppiShot *failure=fixture(failed); [failure startFinished:nil]; [failure terminate];
  assert(failed.stops==1 && ![failure stoppedSuccessfully]);

  FakeShotStream *notStarted=[FakeShotStream new]; LoppiShot *bad=fixture(notStarted);
  [bad startFinished:failed.stopError]; [bad terminate];
  assert(notStarted.stops==0 && bad.finished && !bad.started && bad.stream==nil);
  puts("Native screenshot lifecycle: late start, one stop, stop acknowledgement and failures passed");
  return 0;
} }
