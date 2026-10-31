// Exercise the production SCK discovery boundary without permission prompts or capture.
#define CGPreflightScreenCaptureAccess loppi_test_preflight
#include "../crates/loppi-core/native/system_audio.m"
#include <objc/runtime.h>
#include <assert.h>

static bool granted = true;
static int discovery_code = 0;
bool loppi_test_preflight(void) { return granted; }

static void content_result(id receiver, SEL selector, BOOL exclude, BOOL onscreen,
                           void (^completion)(SCShareableContent *, NSError *)) {
  (void)receiver; (void)selector; (void)exclude; (void)onscreen;
  completion(nil, discovery_code ? [NSError errorWithDomain:SCStreamErrorDomain
                                    code:discovery_code userInfo:nil] : nil);
}
static void audio(void *ctx, const float *samples, uint32_t frames, uint32_t rate,
                  uint32_t channels, uint64_t start) {
  (void)ctx; (void)samples; (void)frames; (void)rate; (void)channels; (void)start;
  assert(false); // Discovery failure must never start capture.
}
int main(void) {
  @autoreleasepool {
    Method method = class_getClassMethod([SCShareableContent class],
      @selector(getShareableContentExcludingDesktopWindows:onScreenWindowsOnly:completionHandler:));
    IMP original = method_setImplementation(method, (IMP)content_result);
    int context = 1, stage = -1, native = -1;
    for (int code = -3801; code <= -3800; code++) {
      discovery_code = code;
      assert(loppi_legacy_system_audio_start(&context, audio, &stage, &native) == NULL);
      assert(stage == 2 && native == code);
    }
    discovery_code = 0;
    assert(loppi_legacy_system_audio_start(&context, audio, &stage, &native) == NULL);
    assert(stage == 2 && native == SCStreamErrorNoCaptureSource);
    granted = false;
    assert(loppi_legacy_system_audio_start(&context, audio, &stage, &native) == NULL);
    assert(stage == 7 && native == 0);
    method_setImplementation(method, original);
  }
  return 0;
}
