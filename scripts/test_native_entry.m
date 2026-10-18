// Exercise the exact native mask, including the stationary-hover regression.
#import "../src-tauri/native/entry.m"
#include <assert.h>

// Drive the production pointer/layout policy without moving the system cursor
// or opening an application window. Only screen geometry/animation is stubbed.
@interface EntryTestWindow : NSObject
@property NSRect frame;
@property BOOL ignoresMouseEvents;
@property BOOL isVisible;
@end
@implementation EntryTestWindow
@end
@interface EntryTest : HearEntry
@property NSPoint testPoint;
@end
@implementation EntryTest
- (HearEntryGeometry)layoutAnimated:(BOOL)animated {
  (void)animated;
  HearEntryGeometry g=self.view==0
    ? (HearEntryGeometry){self.feedback ? 280 : 44,self.feedback ? 80 : 32,32,self.feedback ? 126 : 8,2}
    : (HearEntryGeometry){412,160,32,20,56};
  self.geometry=g;
  ((EntryTestWindow *)self.window).frame=NSMakeRect(self.view==0 ? 847-g.width/2 : 529,956-g.height,g.width,g.height);
  [self updatePointer:nil at:self.testPoint];
  return g;
}
@end
static int enters, releases, dismissals;
static void eventReceived(int kind,int value) {
  if(kind==1 && value) enters++;
  if(kind==7) releases++;
  if(kind==2) dismissals++;
}
static void testReopening(void) {
  EntryTestWindow *window=[EntryTestWindow new];
  EntryTest *e=[EntryTest new];
  e.window=(NSWindow *)window; e.callback=eventReceived;
  void *handle=(__bridge void *)window;
  objc_setAssociatedObject(window,HearEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  NSEvent *move=[NSEvent mouseEventWithType:NSEventTypeMouseMoved location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil eventNumber:0 clickCount:0 pressure:0];
  NSEvent *click=[NSEvent mouseEventWithType:NSEventTypeLeftMouseDown location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil eventNumber:0 clickCount:1 pressure:1];
  NSPoint rabbit=NSMakePoint(847,940), action=NSMakePoint(830,860);
  for(int feedback=0;feedback<=1;feedback++) {
    // Mark/screenshot/comment/cancel all dismiss through the same layout path.
    e.testPoint=action;
    hear_entry_layout(handle,0,1,0,0,0);
    hear_entry_layout(handle,0,0,feedback,0,1);
    assert(!e.inhibited && !e.inside && window.ignoresMouseEvents);
    int before=enters;
    e.testPoint=rabbit;
    [e updatePointer:move at:rabbit];
    assert(e.inside && enters==before+1 && !window.ignoresMouseEvents);
    // A delayed frontend suppress flag must not rearm on toast resize/expiry.
    hear_entry_layout(handle,0,0,!feedback,0,1);
    assert(!e.inhibited && e.inside && enters==before+1);
  }
  // Escape while stationary over the original trigger does not spring open.
  e.testPoint=rabbit;
  hear_entry_layout(handle,0,1,0,0,0);
  hear_entry_layout(handle,0,0,0,0,1);
  int before=enters;
  assert(e.inhibited && !e.inside);
  hear_entry_layout(handle,0,0,1,0,1);
  [e updatePointer:move at:rabbit];
  assert(e.inhibited && enters==before);
  // A physical click is an explicit reopen, without any leave first.
  [e updatePointer:click at:rabbit];
  assert(!e.inhibited && e.inside && enters==before+1);
  // Keyboard/AX click has no native mouse-down; the requested panel clears it.
  hear_entry_layout(handle,0,1,0,0,0);
  hear_entry_layout(handle,0,0,0,0,1);
  assert(e.inhibited);
  hear_entry_layout(handle,0,1,0,0,0);
  int dismissed=dismissals;
  [e updatePointer:click at:action];
  assert(!e.inhibited && dismissals==dismissed);
  // Exit/re-enter only the small trigger, still inside the old panel bounds.
  hear_entry_layout(handle,0,0,0,0,1);
  [e updatePointer:move at:NSMakePoint(800,940)];
  before=enters;
  [e updatePointer:move at:rabbit];
  assert(!e.inhibited && enters==before+1);
  assert(releases>0);
  // Transparent feedback never intercepts another application's click.
  hear_entry_layout(handle,0,0,1,0,0);
  [e updatePointer:click at:NSMakePoint(847,900)];
  assert(window.ignoresMouseEvents);
}
int main(void) {
  testReopening();
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
  puts("Native entry: action/feedback reopen, stationary dismissal, click/keyboard, hit regions and animation passed");
  return 0;
}
