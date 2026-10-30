#import <Foundation/Foundation.h>
#import <IOKit/ps/IOPowerSources.h>
#import <IOKit/ps/IOPSKeys.h>
#import <IOKit/pwr_mgt/IOPMLib.h>
#import <IOKit/IOMessage.h>
#include <libproc.h>
#include <stdatomic.h>
#include <stdint.h>
#include <time.h>

typedef struct {
  uint32_t available, sleeping;
  uint64_t generation;
  int64_t sleep_ms;
} HearPower;
static io_connect_t powerConnection;
static IONotificationPortRef powerPort;
static io_object_t powerNotifier;
static dispatch_queue_t powerQueue;
static atomic_uint powerSleeping;
static atomic_uint_fast64_t powerGeneration;
static atomic_int_fast64_t powerSleepMs;
static void power_changed(void *context, io_service_t service, natural_t message, void *argument) {
  (void)context; (void)service;
  if (message == kIOMessageSystemWillSleep) {
    struct timespec now;
    clock_gettime(CLOCK_REALTIME, &now);
    atomic_store(&powerSleepMs, (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000);
    atomic_store(&powerSleeping, 1);
    atomic_fetch_add(&powerGeneration, 1);
  } else if (message == kIOMessageSystemHasPoweredOn) {
    // Legacy application notifications wait for full wake. An early wake or
    // DarkWake is not evidence that microphone/output hardware is available.
    atomic_store(&powerSleeping, 0);
    atomic_fetch_add(&powerGeneration, 1);
  }
  if (message == kIOMessageCanSystemSleep || message == kIOMessageSystemWillSleep)
    IOAllowPowerChange(powerConnection, (intptr_t)argument);
}
void hear_power_snapshot(HearPower *out) {
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    powerConnection = IORegisterForSystemPower(NULL, &powerPort, power_changed, &powerNotifier);
    if (powerConnection && powerPort) {
      powerQueue = dispatch_queue_create("app.hear.power", DISPATCH_QUEUE_SERIAL);
      IONotificationPortSetDispatchQueue(powerPort, powerQueue);
    }
  });
  out->available = powerConnection && powerPort;
  out->sleeping = atomic_load(&powerSleeping);
  out->generation = atomic_load(&powerGeneration);
  out->sleep_ms = atomic_load(&powerSleepMs);
}

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
  // Unlike proc_listpids, this convenience API already returns a PID count.
  int count = proc_listchildpids(getpid(), pids, sizeof(pids));
  for (int i=0;i<count && i<64;i++) total += hear_process_footprint(pids[i]);
  return total;
}
