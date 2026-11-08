#import <Foundation/Foundation.h>
#include <stdint.h>
#include <math.h>
#include <string.h>

static NSDate *schedule_boundary(NSCalendar *calendar, NSDate *date, int minute) {
  NSDate *boundary = [calendar dateBySettingHour:minute / 60 minute:minute % 60 second:0
                                         ofDate:date options:NSCalendarMatchNextTime | NSCalendarMatchFirst];
  if (boundary && [calendar isDate:boundary inSameDayAsDate:date]) return boundary;

  // Some gaps (including Lord Howe's 30-minute shift) make Foundation
  // return tomorrow's matching hour. Use the first valid local minute instead.
  for (int candidate = minute + 1; candidate < 1440; candidate++) {
    boundary = [calendar dateBySettingHour:candidate / 60 minute:candidate % 60 second:0
                                    ofDate:date options:NSCalendarMatchStrictly | NSCalendarMatchFirst];
    if (boundary && [calendar isDate:boundary inSameDayAsDate:date]) return boundary;
  }
  NSDate *next = [calendar dateByAddingUnit:NSCalendarUnitDay value:1 toDate:date options:0];
  return next ? [calendar startOfDayForDate:next] : nil;
}

// Calendar conversion only. Core owns policy, persistence and all actions.
int loppi_schedule_window(int64_t now_ms, int start, int stop, int weekdays, const char *zone_name,
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
    if (!zone || start < 0 || start >= 1440 || stop < 0 || stop >= 1440 || start == stop ||
        weekdays < 1 || weekdays > 127) return 0;
    const char *name = zone.name.UTF8String;
    if (!name || strlen(name) + 1 > capacity) return 0;
    strcpy(zone_out, name);
    NSCalendar *calendar = [[NSCalendar alloc] initWithCalendarIdentifier:NSCalendarIdentifierGregorian];
    calendar.timeZone = zone;
    NSDate *now = [NSDate dateWithTimeIntervalSince1970:(double)now_ms / 1000.0];
    NSDate *day = [calendar startOfDayForDate:now];
    int64_t before = INT64_MIN, after = INT64_MAX;
    int before_order = 0, after_order = 0;
    int radius = weekdays == 127 ? 2 : 8;
    for (int offset = -radius; offset <= radius; offset++) {
      NSDate *date = [calendar dateByAddingUnit:NSCalendarUnitDay value:offset toDate:day options:0];
      int weekday = ((int)[calendar component:NSCalendarUnitWeekday fromDate:date] + 5) % 7;
      for (int action = 0; action < 2; action++) {
        int origin = !action && stop < start ? (weekday + 6) % 7 : weekday;
        if ((weekdays & (1 << origin)) == 0) continue;
        int minute = action ? start : stop;
        NSDate *boundary = schedule_boundary(calendar, date, minute);
        if (!boundary) return 0;
        int64_t at = (int64_t)llround(boundary.timeIntervalSince1970 * 1000.0);
        int wall_order = offset * 1440 + minute;
        // A DST gap can map both wall times to one instant. Only the last
        // intended wall-time action wins; repeated times use the first fold.
        if (at <= now_ms && (at > before || (at == before && wall_order > before_order))) {
          before = at; before_order = wall_order; *previous_start = action;
        }
        if (at > now_ms && (at < after || (at == after && wall_order > after_order))) {
          after = at; after_order = wall_order; *next_start = action;
        }
      }
    }
    if (before == INT64_MIN || after == INT64_MAX) return 0;
    *previous_ms = before; *next_ms = after;
    return 1;
  }
}
