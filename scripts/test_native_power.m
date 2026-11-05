// Execute production callbacks/snapshots with injected OS reads; never sleep host.
#define IOAllowPowerChange loppi_test_allow_power
#define IORegisterForSystemPower loppi_test_register_power
#define IONotificationPortSetDispatchQueue loppi_test_set_queue
#define sysctlbyname loppi_test_sysctl
#define mach_absolute_time loppi_test_ticks
#define SCDynamicStoreCopyValue loppi_test_capabilities
#include "../crates/loppi-core/native/resources.m"
#include <assert.h>
static int acknowledgements, reads, sysctl_error;
static bool short_read, missing, malformed, race_sleep;
static uint64_t tick = 100, wake_tick = 50;
static int64_t caps = 31;
IOReturn loppi_test_allow_power(io_connect_t c, intptr_t t) { acknowledgements++; return kIOReturnSuccess; }
io_connect_t loppi_test_register_power(void *r, IONotificationPortRef *p, IOServiceInterestCallback cb, io_object_t *n) { *p=(IONotificationPortRef)1; *n=1; return 1; }
void loppi_test_set_queue(IONotificationPortRef p, dispatch_queue_t q) {}
uint64_t loppi_test_ticks(void) { return tick; }
int loppi_test_sysctl(const char *name, void *out, size_t *size, void *in, size_t in_size) {
 assert(strcmp(name,"kern.wake_abs_time")==0);
 memcpy(out,&wake_tick,sizeof(wake_tick));
 if(short_read) *size=1;
 return sysctl_error;
}
CFPropertyListRef loppi_test_capabilities(SCDynamicStoreRef s, CFStringRef key) {
 assert(CFEqual(key,CFSTR("State:/IOKit/SystemPowerCapabilities")));
 reads++;
 if(race_sleep) { race_sleep=false; tick+=100; power_changed(NULL,0,kIOMessageSystemWillSleep,NULL); }
 if(missing) return NULL;
 if(malformed) return CFRetain(CFSTR("31"));
 return CFNumberCreate(NULL,kCFNumberSInt64Type,&caps);
}
static LoppiPower snapshot(void) { LoppiPower p; loppi_power_snapshot(&p); return p; }
static void sleep_now(void) { tick+=100; power_changed(NULL,0,kIOMessageSystemWillSleep,NULL); }
int main(void) {
 power_changed(NULL,0,kIOMessageCanSystemSleep,NULL);
 assert(acknowledgements==1 && snapshot().sleeping==0);
 sleep_now();
 LoppiPower asleep=snapshot();
 assert(acknowledgements==2 && asleep.sleeping==1 && asleep.sleep_ms>0);
 // Prior full capabilities cannot undo a just-delivered WillSleep.
 assert(reads==0);
 power_changed(NULL,0,kIOMessageSystemWillPowerOn,NULL);
 assert(snapshot().sleeping==1 && snapshot().generation==asleep.generation);
 power_changed(NULL,0,kIOMessageSystemHasPoweredOn,NULL);
 assert(snapshot().sleeping==0 && snapshot().generation==asleep.generation+1);
 // Duplicate/late normal notification cannot restart healthy sources again.
 power_changed(NULL,0,kIOMessageSystemHasPoweredOn,NULL);
 assert(snapshot().generation==asleep.generation+1);
 // Drop HasPoweredOn entirely. Even a very short actual sleep can recover.
 sleep_now(); asleep=snapshot(); wake_tick=tick+1;
 for(int64_t dark=0;dark<7;dark++) {
   caps=dark;
   for(int i=0;i<20;i++) assert(snapshot().sleeping==1);
 }
 caps=-1; assert(snapshot().sleeping==1);
 caps=31; sysctl_error=-1; assert(snapshot().sleeping==1); sysctl_error=0;
 short_read=true; assert(snapshot().sleeping==1); short_read=false;
 missing=true; assert(snapshot().sleeping==1); missing=false;
 malformed=true; assert(snapshot().sleeping==1); malformed=false;
 // Full capability observed after this kernel wake is the fallback evidence.
 LoppiPower awake=snapshot();
 assert(awake.sleeping==0 && awake.generation==asleep.generation+1 && awake.sleep_ms==asleep.sleep_ms);
 int prior_reads=reads;
 for(int i=0;i<20;i++) assert(snapshot().generation==awake.generation);
 assert(reads==prior_reads); // No polling of capabilities while awake/display off.
 power_changed(NULL,0,kIOMessageSystemHasPoweredOn,NULL);
 assert(snapshot().generation==awake.generation);
 // A new sleep delivered during an OS read supersedes the old query result.
 sleep_now(); wake_tick=tick+1; race_sleep=true;
 asleep=snapshot(); assert(asleep.sleeping==1);
 assert(snapshot().sleeping==1); // New boundary rejects prior kernel wake.
 wake_tick=tick+1; caps=9; assert(snapshot().sleeping==1); // CPU+network DarkWake
 caps=31; assert(snapshot().sleeping==0);
 puts("native power: normal/missed/duplicate wake, short sleep, DarkWake, stale/failed reads and new-sleep race passed");
 return 0;
}
