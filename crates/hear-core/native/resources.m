#import <Foundation/Foundation.h>
#import <IOKit/ps/IOPowerSources.h>
#import <IOKit/ps/IOPSKeys.h>
#include <libproc.h>
#include <stdatomic.h>
#include <stdint.h>

static _Atomic unsigned pressure = 0;
static dispatch_source_t pressureSource;
typedef struct {
  uint32_t memory_pressure, thermal, low_power, on_battery;
  uint64_t physical_memory;
} HearResources;
void hear_resource_snapshot(HearResources *out) {
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    pressureSource = dispatch_source_create(DISPATCH_SOURCE_TYPE_MEMORYPRESSURE, 0,
      DISPATCH_MEMORYPRESSURE_NORMAL | DISPATCH_MEMORYPRESSURE_WARN | DISPATCH_MEMORYPRESSURE_CRITICAL,
      dispatch_get_global_queue(QOS_CLASS_UTILITY, 0));
    if (pressureSource) {
      dispatch_source_set_event_handler(pressureSource, ^{
        unsigned long flags = dispatch_source_get_data(pressureSource);
        atomic_store(&pressure, flags & DISPATCH_MEMORYPRESSURE_CRITICAL ? 2 : (flags & DISPATCH_MEMORYPRESSURE_WARN ? 1 : 0));
      });
      dispatch_resume(pressureSource);
    }
  });
  @autoreleasepool {
    NSProcessInfo *info = NSProcessInfo.processInfo;
    out->memory_pressure = atomic_load(&pressure);
    out->thermal = (uint32_t)info.thermalState;
    out->low_power = info.lowPowerModeEnabled;
    out->physical_memory = info.physicalMemory;
    CFTypeRef sources = IOPSCopyPowerSourcesInfo();
    CFStringRef kind = sources ? IOPSGetProvidingPowerSourceType(sources) : NULL;
    out->on_battery = kind && CFEqual(kind, CFSTR(kIOPSBatteryPowerValue));
    if (sources) CFRelease(sources);
  }
}
uint64_t hear_process_footprint(int pid) {
  struct rusage_info_v2 usage = {0};
  return proc_pid_rusage(pid,RUSAGE_INFO_V2,(rusage_info_t *)&usage)==0 ? usage.ri_phys_footprint : 0;
}
uint64_t hear_background_footprint(void) {
  uint64_t total = hear_process_footprint(getpid());
  // Core has bounded direct children (two sources and three model roles).
  int pids[64] = {0};
  int bytes = proc_listchildpids(getpid(), pids, sizeof(pids));
  for (int i=0;i<bytes/(int)sizeof(int) && i<64;i++) total += hear_process_footprint(pids[i]);
  return total;
}
