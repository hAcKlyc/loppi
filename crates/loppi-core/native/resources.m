#import <Foundation/Foundation.h>
#import <IOKit/ps/IOPowerSources.h>
#import <IOKit/ps/IOPSKeys.h>
#import <IOKit/pwr_mgt/IOPMLib.h>
#import <IOKit/IOMessage.h>
#include <libproc.h>
#include <stdatomic.h>
#include <stdint.h>
#include <time.h>
#include <pthread.h>
#include <sys/sysctl.h>
#include <mach/mach_time.h>
#import <SystemConfiguration/SystemConfiguration.h>

typedef struct {
  uint32_t available, sleeping;
  uint64_t generation;
  int64_t sleep_ms;
} LoppiPower;
static io_connect_t powerConnection;
static IONotificationPortRef powerPort;
static io_object_t powerNotifier;
static dispatch_queue_t powerQueue;
static atomic_uint powerSleeping;
static atomic_uint_fast64_t powerGeneration;
static atomic_int_fast64_t powerSleepMs;
// Both Core polling threads and the IOKit dispatch queue access this snapshot.
static pthread_mutex_t powerLock = PTHREAD_MUTEX_INITIALIZER;
static uint64_t powerSleepTick;

static bool full_wake_after(uint64_t sleepTick) {
  uint64_t wakeTick = 0;
  size_t size = sizeof(wakeTick);
  if (sysctlbyname("kern.wake_abs_time", &wakeTick, &size, NULL, 0) != 0 ||
      size != sizeof(wakeTick) || wakeTick <= sleepTick) return false;
  // powerd publishes CPU/Video/Audio after SYSTEM_DID_WAKE with Graphics gain;
  // DarkWake lacks Video/Audio. This is an OS implementation key, not a stable
  // public schema: unavailable or malformed values must fail closed.
  CFPropertyListRef value = SCDynamicStoreCopyValue(NULL,
      CFSTR("State:/IOKit/SystemPowerCapabilities"));
  int64_t capabilities = 0;
  bool full = value && CFGetTypeID(value) == CFNumberGetTypeID() &&
      !CFNumberIsFloatType((CFNumberRef)value) &&
      CFNumberGetValue((CFNumberRef)value, kCFNumberSInt64Type, &capabilities) &&
      capabilities >= 0 && (capabilities & 7) == 7; // powerd CPU=1, Video=2, Audio=4
  if (value) CFRelease(value);
  return full;
}
static void power_changed(void *context, io_service_t service, natural_t message, void *argument) {
  (void)context; (void)service;
  pthread_mutex_lock(&powerLock);
  if (message == kIOMessageSystemWillSleep) {
    struct timespec now;
    clock_gettime(CLOCK_REALTIME, &now);
    atomic_store(&powerSleepMs, (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000);
    powerSleepTick = mach_absolute_time();
    atomic_store(&powerSleeping, 1);
    atomic_fetch_add(&powerGeneration, 1);
  } else if (message == kIOMessageSystemHasPoweredOn) {
    // Legacy application notifications wait for full wake. An early wake or
    // DarkWake is not evidence that microphone/output hardware is available.
    if (atomic_exchange(&powerSleeping, 0))
      atomic_fetch_add(&powerGeneration, 1);
  }
  pthread_mutex_unlock(&powerLock);
  if (message == kIOMessageCanSystemSleep || message == kIOMessageSystemWillSleep)
    IOAllowPowerChange(powerConnection, (intptr_t)argument);
}
void loppi_power_snapshot(LoppiPower *out) {
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    powerConnection = IORegisterForSystemPower(NULL, &powerPort, power_changed, &powerNotifier);
    if (powerConnection && powerPort) {
      powerQueue = dispatch_queue_create("app.loppi.power", DISPATCH_QUEUE_SERIAL);
      IONotificationPortSetDispatchQueue(powerPort, powerQueue);
    }
  });
  pthread_mutex_lock(&powerLock);
  uint64_t generation = atomic_load(&powerGeneration);
  uint64_t sleepTick = powerSleepTick;
  bool sleeping = atomic_load(&powerSleeping);
  pthread_mutex_unlock(&powerLock);
  // Do not hold the lock over OS queries: WillSleep must be acknowledged promptly.
  bool recovered = sleeping && full_wake_after(sleepTick);
  pthread_mutex_lock(&powerLock);
  if (recovered && atomic_load(&powerSleeping) &&
      atomic_load(&powerGeneration) == generation) {
    atomic_store(&powerSleeping, 0);
    atomic_fetch_add(&powerGeneration, 1);
  }
  out->available = powerConnection && powerPort;
  out->sleeping = atomic_load(&powerSleeping);
  out->generation = atomic_load(&powerGeneration);
  out->sleep_ms = atomic_load(&powerSleepMs);
  pthread_mutex_unlock(&powerLock);
}

static _Atomic unsigned pressure = 0;
static dispatch_source_t pressureSource;
typedef struct {
  uint32_t memory_pressure, thermal, low_power, on_battery;
  uint64_t physical_memory;
} LoppiResources;
void loppi_resource_snapshot(LoppiResources *out) {
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
uint64_t loppi_process_footprint(int pid) {
  struct rusage_info_v2 usage = {0};
  return proc_pid_rusage(pid,RUSAGE_INFO_V2,(rusage_info_t *)&usage)==0 ? usage.ri_phys_footprint : 0;
}
uint64_t loppi_background_footprint(void) {
  uint64_t total = loppi_process_footprint(getpid());
  // Core has bounded direct children (two sources and three model roles).
  int pids[64] = {0};
  // Unlike proc_listpids, this convenience API already returns a PID count.
  int count = proc_listchildpids(getpid(), pids, sizeof(pids));
  for (int i=0;i<count && i<64;i++) total += loppi_process_footprint(pids[i]);
  return total;
}
