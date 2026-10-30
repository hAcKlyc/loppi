#import <AppKit/AppKit.h>
#include <stdint.h>
// Only process identities cross the preparation barrier; never titles/paths.
static NSArray<NSNumber *> *hear_gui_pids(void) {
  NSMutableArray<NSNumber *> *pids=[NSMutableArray array];
  for (NSRunningApplication *app in [NSRunningApplication runningApplicationsWithBundleIdentifier:@"app.hear.desktop"])
    if (!app.terminated && app.processIdentifier>1) [pids addObject:@(app.processIdentifier)];
  return [pids sortedArrayUsingSelector:@selector(compare:)];
}
static uint64_t hear_exclusion_hash(NSArray<NSNumber *> *ids) {
  uint64_t value=1469598103934665603ULL;
  for (NSNumber *identifier in ids) { value^=identifier.unsignedLongLongValue; value*=1099511628211ULL; }
  return value ?: 1;
}
static uint64_t hear_gui_generation(void) { return hear_exclusion_hash(hear_gui_pids()); }
