// Exercise the exact native mask, including the stationary-hover regression.
#import "../src-tauri/native/entry.m"
#include <assert.h>

// Drive the production pointer/layout policy without moving the system cursor
// or opening an application window. Only screen geometry/animation is stubbed.
@interface EntryTestWindow : NSObject
@property NSRect frame;
@property BOOL ignoresMouseEvents;
@property BOOL isVisible;
@property BOOL isKeyWindow;
- (void)makeKeyAndOrderFront:(id)sender;
- (void)resignKeyWindow;
@end
@implementation EntryTestWindow
- (void)makeKeyAndOrderFront:(id)sender { (void)sender; }
- (void)resignKeyWindow { self.isKeyWindow=NO; }
@end
@interface EntryTest : HearEntry
@property NSPoint testPoint;
@property BOOL keyboard;
@property double lastDuration;
@end
@implementation EntryTest
- (BOOL)ownsKeyboard { return self.keyboard; }
- (HearEntryGeometry)layoutAnimated:(double)duration {
  self.lastDuration=duration;
  HearEntryGeometry g=self.view==0
    ? (HearEntryGeometry){self.feedback ? 280 : 44,self.feedback ? 80 : 32,32,self.feedback ? 126 : 8,2}
    : (HearEntryGeometry){412,160,32,20,56};
  self.geometry=g;
  ((EntryTestWindow *)self.window).frame=NSMakeRect(self.view==0 ? 847-g.width/2 : 529,956-g.height,g.width,g.height);
  self.targetFrame=self.window.frame;
  [self updatePointer:nil at:self.testPoint];
  return g;
}
@end
static int enters, releases, dismissals, presented, focused;
static uint64_t confirmedGeneration;
static void eventReceived(int kind,int value,uint64_t generation) {
  if(kind==8) { presented++; confirmedGeneration=generation; }
  if(kind==9) { focused++; confirmedGeneration=generation; }
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
    hear_entry_layout(handle,0,1,0,0,0,0);
    hear_entry_layout(handle,0,0,feedback,0,1,0);
    assert(!e.inhibited && !e.inside && window.ignoresMouseEvents);
    int before=enters;
    e.testPoint=rabbit;
    [e updatePointer:move at:rabbit];
    assert(e.inside && enters==before+1 && !window.ignoresMouseEvents);
    // A delayed frontend suppress flag must not rearm on toast resize/expiry.
    hear_entry_layout(handle,0,0,!feedback,0,1,0);
    assert(!e.inhibited && e.inside && enters==before+1);
  }
  // Escape while stationary over the original trigger does not spring open.
  e.testPoint=rabbit;
  hear_entry_layout(handle,0,1,0,0,0,0);
  hear_entry_layout(handle,0,0,0,0,1,0);
  int before=enters;
  assert(e.inhibited && !e.inside);
  hear_entry_layout(handle,0,0,1,0,1,0);
  [e updatePointer:move at:rabbit];
  assert(e.inhibited && enters==before);
  // A physical click is an explicit reopen, without any leave first.
  [e updatePointer:click at:rabbit];
  assert(!e.inhibited && e.inside && enters==before+1);
  // Keyboard/AX click has no native mouse-down; the requested panel clears it.
  hear_entry_layout(handle,0,1,0,0,0,0);
  hear_entry_layout(handle,0,0,0,0,1,0);
  assert(e.inhibited);
  hear_entry_layout(handle,0,1,0,0,0,0);
  int dismissed=dismissals;
  [e updatePointer:click at:action];
  assert(!e.inhibited && dismissals==dismissed);
  // Exit/re-enter only the small trigger, still inside the old panel bounds.
  hear_entry_layout(handle,0,0,0,0,1,0);
  [e updatePointer:move at:NSMakePoint(800,940)];
  before=enters;
  [e updatePointer:move at:rabbit];
  assert(!e.inhibited && enters==before+1);
  assert(releases>0);
  // Transparent feedback never intercepts another application's click.
  hear_entry_layout(handle,0,0,1,0,0,0);
  [e updatePointer:click at:NSMakePoint(847,900)];
  assert(window.ignoresMouseEvents);
}
static void testCompletionAndFocus(void) {
  EntryTestWindow *window=[EntryTestWindow new];
  EntryTest *e=[EntryTest new]; e.window=(NSWindow *)window; e.callback=eventReceived;
  void *handle=(__bridge void *)window;
  objc_setAssociatedObject(window,HearEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  e.mode=0; e.view=1; e.layoutGeneration=7; e.layoutSerial=2;
  [e finishLayout:1 generation:7]; // canceled native animation
  [e finishLayout:2 generation:6]; // old UI request
  assert(presented==0);
  [e finishLayout:2 generation:7];
  assert(presented==1 && confirmedGeneration==7);
  e.view=0; [e finishLayout:2 generation:7];
  assert(presented==1);
  e.view=2;
  assert(!hear_entry_focus(handle,1,0,8)); // requesting focus is not success
  assert(focused==0);
  e.keyboard=YES;
  assert([e confirmFocus] && focused==1 && confirmedGeneration==8);
  assert([e confirmFocus] && focused==1); // one confirmation per intent
  hear_entry_focus(handle,0,0,9);
  assert(![e confirmFocus] && focused==1); // late notification after dismissal
}
static void testOutsideClose(void) {
  EntryTestWindow *window=[EntryTestWindow new]; window.isVisible=YES;
  EntryTest *e=[EntryTest new]; e.window=(NSWindow *)window; e.callback=eventReceived;
  void *handle=(__bridge void *)window;
  objc_setAssociatedObject(window,HearEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  e.testPoint=NSMakePoint(100,500);
  hear_entry_layout(handle,0,1,0,0,0,1);
  NSEvent *click=[NSEvent mouseEventWithType:NSEventTypeLeftMouseDown location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil eventNumber:0 clickCount:1 pressure:1];
  int before=dismissals;
  [e updatePointer:click at:e.testPoint];
  assert(dismissals==before+1 && !e.commentFocus && window.ignoresMouseEvents);
  // Frontend consumes native cancellation and requests its new presentation.
  hear_entry_layout(handle,0,0,0,0,1,2);
  double expected=NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion ? 0 : 0.14;
  assert(e.lastDuration==expected && e.view==0);
}
static void testClosingHit(void) {
  EntryTestWindow *window=[EntryTestWindow new];
  HearEntry *e=[HearEntry new]; e.window=(NSWindow *)window;
  e.mode=0; e.view=0;
  e.geometry=(HearEntryGeometry){44,32,32,8,2,32};
  e.targetFrame=NSMakeRect(825,924,44,32);
  window.frame=NSMakeRect(529,796,412,160); // visual tail is still expanded
  assert([e hit:NSMakePoint(847,940)]);
  assert(![e hit:NSMakePoint(551,940)]); // no hit at old-frame + compact offset
  assert(![e hit:NSMakePoint(830,860)]); // dismissed action passes through now
}
static void testTopLayouts(void) {
  NSRect screen=NSMakeRect(-1920,200,1920,1080), frame;
  HearEntryGeometry g=topLayout(screen,0,NSZeroRect,NSZeroRect,0,NO,NO,&frame);
  assert(g.band==32 && g.height==32 && NSMaxY(frame)==NSMaxY(screen));
  assert(NSMidX(frame)==NSMidX(screen));
  g=topLayout(screen,0,NSZeroRect,NSZeroRect,1,NO,NO,&frame);
  assert(g.band==0 && g.top_band==0 && g.height==128 && g.rabbit_y==24);
  assert(NSMaxY(frame)==NSMaxY(screen) && NSMidX(frame)==NSMidX(screen));
  assert(hitRegion(g,0,1,NO,NSMakePoint(206,16)));
  g=topLayout(screen,0,NSZeroRect,NSZeroRect,2,NO,NO,&frame);
  assert(g.band==0 && g.height==152);
  screen=NSMakeRect(0,0,1470,956);
  NSRect left=NSMakeRect(0,924,646,32), right=NSMakeRect(825,924,645,32);
  g=topLayout(screen,32,left,right,1,NO,NO,&frame);
  assert(g.band==32 && g.top_band==32 && g.height==160 && NSMidX(frame)==735.5);
  g=topLayout(screen,32,NSZeroRect,right,1,NO,NO,&frame);
  assert(g.band==0 && NSMidX(frame)==735); // incomplete safe areas are not a notch
}
int main(void) {
  NSRect target=NSMakeRect(529,796,412,160);
  NSRect peak=expansionPeak(NSMakeRect(825,924,44,32),target);
  assert(peak.size.width>412 && peak.size.width<420);
  assert(peak.size.height>160 && peak.size.height<164);
  assert(NSMaxY(peak)==NSMaxY(target));
  assert(NSEqualRects(expansionPeak(target,target),target));
  testTopLayouts();
  testClosingHit();
  testOutsideClose();
  testCompletionAndFocus();
  testReopening();
  HearEntryGeometry top={412,160,32,20,56};
  // Built-in notch spans 646...825; entry is centered at 735, rabbit at 847.
  // These are coordinates in the expanded window, including the menu-bar band.
  assert(hitRegion(top,0,1,NO,NSMakePoint(318,16)));
  assert(hitRegion(top,0,2,NO,NSMakePoint(318,16)));
  assert(hitRegion(top,0,1,NO,NSMakePoint(200,159)));
  assert(!hitRegion(top,0,1,NO,NSMakePoint(0,159)));
  assert(!hitRegion(top,0,1,NO,NSMakePoint(413,80)));
  assert(motionDuration(0,0,0,1,YES,NO)==0.30);
  assert(!motionDuration(0,0,0,1,YES,YES));
  assert(!motionDuration(0,0,0,1,NO,NO));
  assert(!motionDuration(1,0,0,1,YES,NO));
  assert(!motionDuration(0,1,0,2,YES,NO));
  assert(motionDuration(0,1,0,0,YES,NO)==0.14);
  assert(motionDuration(0,2,0,0,YES,NO)==0.14);
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
