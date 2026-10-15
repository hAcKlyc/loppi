#include <libproc.h>
#include <mach/mach_time.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>

int main(int argc, char **argv) {
  if (argc == 2 && strcmp(argv[1], "--inventory") == 0) {
    int pids[8192] = {0};
    int count = proc_listallpids(pids, sizeof(pids));
    for (int i=0; i<count && i<8192; i++) {
      struct proc_bsdinfo info = {0};
      char path[PROC_PIDPATHINFO_MAXSIZE] = {0};
      if (proc_pidinfo(pids[i], PROC_PIDTBSDINFO, 0, &info, sizeof(info)) != sizeof(info) ||
          proc_pidpath(pids[i], path, sizeof(path)) <= 0) continue;
      printf("%d\t%u\t%llu.%llu\t%s\n", pids[i], info.pbi_ppid,
          (unsigned long long)info.pbi_start_tvsec, (unsigned long long)info.pbi_start_tvusec, path);
    }
    return 0;
  }
  if (argc == 3 && strcmp(argv[1], "--path") == 0) {
    char *end = NULL;
    long pid = strtol(argv[2], &end, 10);
    char path[PROC_PIDPATHINFO_MAXSIZE] = {0};
    if (!end || *end != '\0' || pid <= 0 || pid > INT32_MAX ||
        proc_pidpath((int)pid, path, sizeof(path)) <= 0) {
      return 1;
    }
    puts(path);
    return 0;
  }
  mach_timebase_info_data_t timebase = {0};
  if (mach_timebase_info(&timebase) != KERN_SUCCESS || timebase.denom == 0) {
    return 1;
  }
  for (int i = 1; i < argc; ++i) {
    char *end = NULL;
    long pid = strtol(argv[i], &end, 10);
    if (!end || *end != '\0' || pid <= 0 || pid > INT32_MAX) {
      continue;
    }
    struct rusage_info_v6 usage = {0};
    if (proc_pid_rusage((int)pid, RUSAGE_INFO_V6, (rusage_info_t *)&usage) != 0) {
      memset(&usage, 0, sizeof(usage));
      if (proc_pid_rusage((int)pid, RUSAGE_INFO_V4, (rusage_info_t *)&usage) != 0) {
        memset(&usage, 0, sizeof(usage));
        if (proc_pid_rusage((int)pid, RUSAGE_INFO_V2, (rusage_info_t *)&usage) != 0) continue;
      }
    }
    uint64_t user_ns = (uint64_t)(((__uint128_t)usage.ri_user_time * timebase.numer) / timebase.denom);
    uint64_t system_ns = (uint64_t)(((__uint128_t)usage.ri_system_time * timebase.numer) / timebase.denom);
    printf("%ld,%llu,%llu,%llu,%llu,%llu,%llu,%llu,%llu,%llu\n", pid,
           (unsigned long long)user_ns,
           (unsigned long long)system_ns,
           (unsigned long long)usage.ri_resident_size,
           (unsigned long long)usage.ri_phys_footprint,
           (unsigned long long)usage.ri_diskio_bytesread,
           (unsigned long long)usage.ri_diskio_byteswritten,
           (unsigned long long)usage.ri_energy_nj,
           (unsigned long long)usage.ri_penergy_nj,
           (unsigned long long)usage.ri_lifetime_max_phys_footprint);
  }
  return 0;
}
