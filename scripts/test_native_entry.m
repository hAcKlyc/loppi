// Exercise the exact native mask, including the stationary-hover regression.
#import "../src-tauri/native/entry.m"
#include <assert.h>
int main(void) {
  HearEntryGeometry top={412,160,32,20,56};
  // Built-in notch spans 646...825; entry is centered at 735, rabbit at 847.
  // These are coordinates in the expanded window, including the menu-bar band.
  assert(hitRegion(top,0,1,NO,NSMakePoint(318,16)));
  assert(hitRegion(top,0,2,NO,NSMakePoint(318,16)));
  assert(hitRegion(top,0,1,NO,NSMakePoint(200,159)));
  assert(!hitRegion(top,0,1,NO,NSMakePoint(0,159)));
  assert(!hitRegion(top,0,1,NO,NSMakePoint(413,80)));
  assert(unfolds(0,0,0,1,YES,NO));
  assert(!unfolds(0,0,0,1,YES,YES));
  assert(!unfolds(0,0,0,1,NO,NO));
  assert(!unfolds(1,0,0,1,YES,NO));
  assert(!unfolds(0,1,0,2,YES,NO));
  assert(!unfolds(0,1,0,0,YES,NO));
  // The physical hover point remains inside throughout the native resize.
  // Transparent corners and the not-yet-expanded area remain click-through.
  for(int step=0;step<=20;step++) {
    double progress=step/20.0;
    NSRect frame=NSMakeRect(825+(529-825)*progress,0,44+(412-44)*progress,32+128*progress);
    HearEntryGeometry live=hitGeometry(top,0,1,frame);
    assert(hitRegion(live,0,1,NO,NSMakePoint(847-frame.origin.x,16)));
    assert(!hitRegion(live,0,1,NO,NSMakePoint(0,live.height-1)));
    assert(!hitRegion(live,0,1,NO,NSMakePoint(live.width+1,16)));
  }
  HearEntryGeometry compact={44,32,32,8,2};
  assert(hitRegion(compact,0,0,NO,NSMakePoint(22,16)));
  assert(!hitRegion(compact,0,0,NO,NSMakePoint(0,0)));
  HearEntryGeometry floating={132,230,0,72,170};
  assert(hitRegion(floating,1,1,NO,NSMakePoint(98,196)));
  assert(hitRegion(floating,1,1,NO,NSMakePoint(50,100)));
  assert(!hitRegion(floating,1,1,NO,NSMakePoint(0,200)));
  assert(!hitRegion(floating,1,1,NO,NSMakePoint(1,1)));
  assert(!hitRegion(floating,1,1,NO,NSMakePoint(9,9)));
  assert(!hitRegion(floating,1,1,NO,NSMakePoint(50,180)));
  assert(hitRegion(floating,1,1,YES,NSMakePoint(19.5,200.5)));
  assert(!hitRegion(floating,1,1,NO,NSMakePoint(19.5,200.5)));
  puts("Native entry hit regions: stationary hover, corners and floating mask passed");
  return 0;
}
