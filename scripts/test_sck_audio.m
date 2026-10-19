#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdint.h>
#include <stdio.h>
extern int64_t hear_legacy_system_audio_probe(uint32_t seconds, uint64_t *peak);
int main(void) {
  @autoreleasepool {
    if (!CGPreflightScreenCaptureAccess()) { puts("sck_smoke: permission_not_pregranted"); return 77; }
    uint64_t peak=0;
    int64_t frames=hear_legacy_system_audio_probe(3, &peak);
    printf("sck_smoke: frames=%lld peak=%llu\n", (long long)frames, (unsigned long long)peak);
    return frames > 0 ? 0 : 1;
  }
}
