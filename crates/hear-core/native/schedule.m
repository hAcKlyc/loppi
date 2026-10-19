#import <Foundation/Foundation.h>
#include <stdint.h>
#include <math.h>
#include <string.h>

// Calendar conversion only. Core owns policy, persistence and all actions.
int hear_schedule_window(int64_t now_ms, int start, int stop, const char *zone_name,
                         char *zone_out, size_t capacity, int64_t *previous_ms,
                         int *previous_start, int64_t *next_ms, int *next_start) {
  @autoreleasepool {
    NSTimeZone *zone;
    if (zone_name) {
      zone = [NSTimeZone timeZoneWithName:[NSString stringWithUTF8String:zone_name]];
    } else {
      [NSTimeZone resetSystemTimeZone];
      zone = [NSTimeZone systemTimeZone];
    }
    if (!zone || start < 0 || start >= 1440 || stop < 0 || stop >= 1440 || start == stop) return 0;
    const char *name = zone.name.UTF8String;
    if (!name || strlen(name) + 1 > capacity) return 0;
    strcpy(zone_out, name);
    NSCalendar *calendar = [[NSCalendar alloc] initWithCalendarIdentifier:NSCalendarIdentifierGregorian];
    calendar.timeZone = zone;
    NSDate *now = [NSDate dateWithTimeIntervalSince1970:(double)now_ms / 1000.0];
    NSDate *day = [calendar startOfDayForDate:now];
    int64_t before = INT64_MIN, after = INT64_MAX;
    int before_minute = -1, after_minute = -1;
    for (int offset = -2; offset <= 2; offset++) {
      NSDate *date = [calendar dateByAddingUnit:NSCalendarUnitDay value:offset toDate:day options:0];
      for (int action = 0; action < 2; action++) {
        int minute = action ? start : stop;
        NSDate *boundary = [calendar dateBySettingHour:minute / 60 minute:minute % 60 second:0
                                               ofDate:date options:NSCalendarMatchNextTime | NSCalendarMatchFirst];
        if (!boundary) return 0;
        int64_t at = (int64_t)llround(boundary.timeIntervalSince1970 * 1000.0);
        // A DST gap can map both wall times to one instant. Only the last
        // intended wall-time action wins; repeated times use the first fold.
        if (at <= now_ms && (at > before || (at == before && minute > before_minute))) {
          before = at; before_minute = minute; *previous_start = action;
        }
        if (at > now_ms && (at < after || (at == after && minute > after_minute))) {
          after = at; after_minute = minute; *next_start = action;
        }
      }
    }
    if (before == INT64_MIN || after == INT64_MAX) return 0;
    *previous_ms = before; *next_ms = after;
    return 1;
  }
}
