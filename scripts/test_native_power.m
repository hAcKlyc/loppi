// Drive the production notification reducer without actually sleeping the host.
#define IOAllowPowerChange loppi_test_allow_power
#include "../crates/loppi-core/native/resources.m"
#include <assert.h>
static int acknowledgements;
IOReturn loppi_test_allow_power(io_connect_t connection, intptr_t token) { (void)connection;(void)token;acknowledgements++;return kIOReturnSuccess; }
int main(void) {
 power_changed(NULL,0,kIOMessageCanSystemSleep,NULL);
 assert(acknowledgements==1 && atomic_load(&powerSleeping)==0);
 power_changed(NULL,0,kIOMessageSystemWillSleep,NULL);
 assert(acknowledgements==2 && atomic_load(&powerSleeping)==1 && atomic_load(&powerSleepMs)>0);
 uint64_t generation=atomic_load(&powerGeneration);
 power_changed(NULL,0,kIOMessageSystemWillPowerOn,NULL);
 assert(atomic_load(&powerSleeping)==1 && atomic_load(&powerGeneration)==generation);
 power_changed(NULL,0,kIOMessageSystemHasPoweredOn,NULL);
 assert(atomic_load(&powerSleeping)==0 && atomic_load(&powerGeneration)==generation+1);
 assert(acknowledgements==2);
 puts("native power: no sleep veto, early wake stays suspended, completed wake resumes");
 return 0;
}
