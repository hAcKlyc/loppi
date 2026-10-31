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
- (NSPoint)convertPointToScreen:(NSPoint)point;
@end
@implementation EntryTestWindow
- (void)makeKeyAndOrderFront:(id)sender { (void)sender; }
- (void)resignKeyWindow { self.isKeyWindow=NO; }
- (NSPoint)convertPointToScreen:(NSPoint)point { return NSMakePoint(point.x+self.frame.origin.x,point.y+self.frame.origin.y); }
@end
@interface EntryTest : LoppiEntry
@property NSPoint testPoint;
@property BOOL keyboard;
@property double lastDuration;
@end
@implementation EntryTest
- (BOOL)ownsKeyboard { return self.keyboard; }
- (LoppiEntryGeometry)layoutAnimated:(double)duration {
  self.lastDuration=duration;
  LoppiEntryGeometry g=self.view==0
    ? (LoppiEntryGeometry){self.feedback ? 280 : 44,self.feedback ? 80 : 32,32,self.feedback ? 126 : 8,2}
    : (LoppiEntryGeometry){412,160,32,20,56};
  self.geometry=g;
  ((EntryTestWindow *)self.window).frame=NSMakeRect(self.view==0 ? 847-g.width/2 : 529,956-g.height,g.width,g.height);
  self.targetFrame=self.window.frame;
  [self updatePointer:nil at:self.testPoint];
  return g;
}
@end
static int enters, releases, dismissals, presented, focused, dragStarts, dragEnds, marks;
static __weak LoppiEntry *observedEntry;
static LoppiEntryGeometry dragStartGeometry;
static uint64_t confirmedGeneration;
static void eventReceived(int kind,int value,uint64_t generation) {
  if(kind==5) { if(value) { dragStarts++; dragStartGeometry=observedEntry.geometry; } else dragEnds++; }
  if(kind==10) marks++;
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
  objc_setAssociatedObject(window,LoppiEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  NSEvent *move=[NSEvent mouseEventWithType:NSEventTypeMouseMoved location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil eventNumber:0 clickCount:0 pressure:0];
  NSEvent *click=[NSEvent mouseEventWithType:NSEventTypeLeftMouseDown location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil eventNumber:0 clickCount:1 pressure:1];
  NSPoint rabbit=NSMakePoint(847,940), action=NSMakePoint(830,860);
  for(int feedback=0;feedback<=1;feedback++) {
    // Mark/screenshot/comment/cancel all dismiss through the same layout path.
    e.testPoint=action;
    loppi_entry_layout(handle,0,1,0,0,0,0);
    loppi_entry_layout(handle,0,0,feedback,0,1,0);
    assert(!e.inhibited && !e.inside && window.ignoresMouseEvents);
    int before=enters;
    e.testPoint=rabbit;
    [e updatePointer:move at:rabbit];
    assert(e.inside && enters==before+1 && !window.ignoresMouseEvents);
    // A delayed frontend suppress flag must not rearm on toast resize/expiry.
    loppi_entry_layout(handle,0,0,!feedback,0,1,0);
    assert(!e.inhibited && e.inside && enters==before+1);
  }
  // Escape while stationary over the original trigger does not spring open.
  e.testPoint=rabbit;
  loppi_entry_layout(handle,0,1,0,0,0,0);
  loppi_entry_layout(handle,0,0,0,0,1,0);
  int before=enters;
  assert(e.inhibited && !e.inside);
  loppi_entry_layout(handle,0,0,1,0,1,0);
  [e updatePointer:move at:rabbit];
  assert(e.inhibited && enters==before);
  // A physical click is an explicit reopen, without any leave first.
  [e updatePointer:click at:rabbit];
  assert(!e.inhibited && e.inside && enters==before+1);
  // Keyboard/AX click has no native mouse-down; the requested panel clears it.
  loppi_entry_layout(handle,0,1,0,0,0,0);
  loppi_entry_layout(handle,0,0,0,0,1,0);
  assert(e.inhibited);
  loppi_entry_layout(handle,0,1,0,0,0,0);
  int dismissed=dismissals;
  [e updatePointer:click at:action];
  assert(!e.inhibited && dismissals==dismissed);
  // Exit/re-enter only the small trigger, still inside the old panel bounds.
  loppi_entry_layout(handle,0,0,0,0,1,0);
  [e updatePointer:move at:NSMakePoint(800,940)];
  before=enters;
  [e updatePointer:move at:rabbit];
  assert(!e.inhibited && enters==before+1);
  assert(releases>0);
  // Transparent feedback never intercepts another application's click.
  loppi_entry_layout(handle,0,0,1,0,0,0);
  [e updatePointer:click at:NSMakePoint(847,900)];
  assert(window.ignoresMouseEvents);
}
static void testCompletionAndFocus(void) {
  EntryTestWindow *window=[EntryTestWindow new];
  EntryTest *e=[EntryTest new]; e.window=(NSWindow *)window; e.callback=eventReceived;
  void *handle=(__bridge void *)window;
  objc_setAssociatedObject(window,LoppiEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  e.mode=0; e.view=1; e.layoutGeneration=7; e.layoutSerial=2;
  [e finishLayout:1 generation:7]; // canceled native animation
  [e finishLayout:2 generation:6]; // old UI request
  assert(presented==0);
  [e finishLayout:2 generation:7];
  assert(presented==1 && confirmedGeneration==7);
  e.view=0; [e finishLayout:2 generation:7];
  assert(presented==1);
  e.view=2;
  assert(!loppi_entry_focus(handle,1,0,8)); // requesting focus is not success
  assert(focused==0);
  e.keyboard=YES;
  assert([e confirmFocus] && focused==1 && confirmedGeneration==8);
  assert([e confirmFocus] && focused==1); // one confirmation per intent
  loppi_entry_focus(handle,0,0,9);
  assert(![e confirmFocus] && focused==1); // late notification after dismissal
}
static void testOutsideClose(void) {
  EntryTestWindow *window=[EntryTestWindow new]; window.isVisible=YES;
  EntryTest *e=[EntryTest new]; e.window=(NSWindow *)window; e.callback=eventReceived;
  void *handle=(__bridge void *)window;
  objc_setAssociatedObject(window,LoppiEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  e.testPoint=NSMakePoint(100,500);
  loppi_entry_layout(handle,0,1,0,0,0,1);
  NSEvent *click=[NSEvent mouseEventWithType:NSEventTypeLeftMouseDown location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil eventNumber:0 clickCount:1 pressure:1];
  int before=dismissals;
  [e updatePointer:click at:e.testPoint];
  assert(dismissals==before+1 && !e.commentFocus && window.ignoresMouseEvents);
  // Frontend consumes native cancellation and requests its new presentation.
  loppi_entry_layout(handle,0,0,0,0,1,2);
  double expected=NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion ? 0 : 0.14;
  assert(e.lastDuration==expected && e.view==0);
}
static void testClosingHit(void) {
  EntryTestWindow *window=[EntryTestWindow new];
  LoppiEntry *e=[LoppiEntry new]; e.window=(NSWindow *)window;
  e.mode=0; e.view=0;
  e.geometry=(LoppiEntryGeometry){44,32,32,8,2,32};
  e.targetFrame=NSMakeRect(825,924,44,32);
  window.frame=NSMakeRect(529,796,412,160); // visual tail is still expanded
  assert([e hit:NSMakePoint(847,940)]);
  assert(![e hit:NSMakePoint(551,940)]); // no hit at old-frame + compact offset
  assert(![e hit:NSMakePoint(830,860)]); // dismissed action passes through now
}
static void testTopLayouts(void) {
  NSRect screen=NSMakeRect(-1920,200,1920,1080), frame;
  LoppiEntryGeometry g=topLayout(screen,0,NSZeroRect,NSZeroRect,0,NO,NO,&frame);
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
@interface FloatingTestScreen : NSObject
@property NSRect visibleFrame;
@end
@implementation FloatingTestScreen
@end
@interface FloatingTestEvent : NSObject
@property NSEventType type;
@property NSWindow *window;
@property NSPoint locationInWindow;
@end
@implementation FloatingTestEvent
@end
@interface FloatingTest : LoppiEntry
@property FloatingTestScreen *screen;
@end
@implementation FloatingTest
- (NSScreen *)floatingScreen { return (NSScreen *)self.screen; }
- (void)moveFloatingTo:(NSPoint)anchor { self.anchor=anchor; [self layout]; }
- (LoppiEntryGeometry)layoutAnimated:(double)duration {
  (void)duration;
  NSRect frame;
  self.geometry=floatingLayout(self.screen.visibleFrame,self.anchor,(self.view!=0 || self.feedback) && !self.drag,&frame);
  ((EntryTestWindow *)self.window).frame=frame; self.targetFrame=frame;
  return self.geometry;
}
@end
static void testFloating(void) {
  // Same production geometry on positive and negative-coordinate displays.
  for(int display=0;display<2;display++) {
    NSRect screen=NSMakeRect(display ? -1920 : 0,display ? -300 : 0,1470,900);
    NSPoint candidates[]={NSMakePoint(NSMinX(screen)+100,NSMidY(screen)),NSMakePoint(NSMaxX(screen)-100,NSMidY(screen)),NSMakePoint(NSMidX(screen),NSMinY(screen)+90),NSMakePoint(NSMidX(screen),NSMaxY(screen)-90)};
    for(int edge=0;edge<4;edge++) {
      NSPoint anchor=floatingAnchor(screen,candidates[edge],YES);
      assert(edge!=0 || anchor.x==NSMinX(screen)+34);
      assert(edge!=1 || anchor.x==NSMaxX(screen)-34);
      assert(edge!=2 || anchor.y==NSMinY(screen)+34);
      assert(edge!=3 || anchor.y==NSMaxY(screen)-34);
      NSRect frame;
      LoppiEntryGeometry g=floatingLayout(screen,anchor,YES,&frame);
      assert(NSContainsRect(screen,frame));
      assert(fabs(frame.origin.x+g.rabbit_x+26-anchor.x)<0.001);
      assert(fabs(NSMaxY(frame)-g.rabbit_y-26-anchor.y)<0.001);
      assert(hitRegion(g,1,1,YES,NSMakePoint(g.rabbit_x+26,g.rabbit_y+26)));
      assert(hitRegion(g,1,1,NO,NSMakePoint(60,g.menu_y+80)));
      assert(!hitRegion(g,1,1,NO,NSMakePoint(8,g.menu_y)));
      if(edge==3) assert(g.menu_y==55 && g.rabbit_y==8);
      NSRect compact; LoppiEntryGeometry c=floatingLayout(screen,anchor,NO,&compact);
      assert(c.rabbit_x==8 && c.rabbit_y==8 && NSContainsRect(screen,compact));
    }
  }
  EntryTestWindow *window=[EntryTestWindow new];
  FloatingTest *e=[FloatingTest new]; e.window=(NSWindow *)window; e.callback=eventReceived;
  observedEntry=e;
  e.screen=[FloatingTestScreen new]; e.screen.visibleFrame=NSMakeRect(0,0,1000,800);
  e.mode=1; e.anchor=NSMakePoint(966,400); [e layout];
  FloatingTestEvent *event=[FloatingTestEvent new]; event.window=e.window;
  event.type=NSEventTypeLeftMouseDown;
  event.locationInWindow=NSMakePoint(e.anchor.x-window.frame.origin.x,e.anchor.y-window.frame.origin.y);
  assert([e checkPointer:(NSEvent *)event]); // Event coordinates, not a later hardware cursor sample.
  event.type=NSEventTypeLeftMouseDragged;
  assert([e updatePointer:(NSEvent *)event at:NSMakePoint(963,400)]);
  assert(!e.drag && marks==0 && dragStarts==0);
  event.type=NSEventTypeLeftMouseUp;
  assert([e updatePointer:(NSEvent *)event at:NSMakePoint(963,400)]);
  assert(marks==1 && dragEnds==0 && !e.dragPending);
  e.feedback=YES; [e layout];
  assert(e.geometry.height==230 && e.geometry.rabbit_y==170);
  event.type=NSEventTypeLeftMouseDown;
  assert([e updatePointer:(NSEvent *)event at:e.anchor]);
  event.type=NSEventTypeLeftMouseDragged;
  assert([e updatePointer:(NSEvent *)event at:NSMakePoint(959,400)]);
  assert(e.drag && e.view==0 && dragStarts==1);
  assert(dragStartGeometry.width==68 && dragStartGeometry.height==68);
  assert(dragStartGeometry.rabbit_x==8 && dragStartGeometry.rabbit_y==8);
  assert([e updatePointer:(NSEvent *)event at:NSMakePoint(100,500)]);
  assert(NSEqualPoints(e.anchor,NSMakePoint(100,500)));
  event.type=NSEventTypeLeftMouseUp;
  assert([e updatePointer:(NSEvent *)event at:NSMakePoint(80,510)]);
  assert(!e.drag && !e.dragPending && dragEnds==1 && marks==1);
  assert(NSEqualPoints(e.anchor,NSMakePoint(34,510)));
  // Unrelated releases cannot save again or become marks.
  [e updatePointer:(NSEvent *)event at:NSMakePoint(80,510)];
  assert(dragEnds==1 && marks==1);
  // The last dragged sample can still be below threshold; mouse-up is final.
  e.feedback=NO; e.anchor=NSMakePoint(966,400); [e layout];
  event.type=NSEventTypeLeftMouseDown;
  [e updatePointer:(NSEvent *)event at:e.anchor];
  event.type=NSEventTypeLeftMouseDragged;
  [e updatePointer:(NSEvent *)event at:NSMakePoint(960,400)];
  assert(!e.drag);
  event.type=NSEventTypeLeftMouseUp;
  [e updatePointer:(NSEvent *)event at:NSMakePoint(950,400)];
  assert(marks==1 && dragStarts==2 && dragEnds==2 && !e.dragPending);
  event.type=NSEventTypeLeftMouseDown;
  assert([e updatePointer:(NSEvent *)event at:e.anchor]);
  void *handle=(__bridge void *)window;
  objc_setAssociatedObject(window,LoppiEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  loppi_entry_layout(handle,0,0,0,0,0,1);
  assert(!e.dragPending && !e.drag && marks==1);
}
int main(void) {
  testFloating();
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
  LoppiEntryGeometry top={412,160,32,20,56};
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
    LoppiEntryGeometry live=hitGeometry(top,0,1,frame);
    assert(hitRegion(live,0,1,NO,NSMakePoint(847-frame.origin.x,16)));
    assert(!hitRegion(live,0,1,NO,NSMakePoint(0,live.height-1)));
    assert(!hitRegion(live,0,1,NO,NSMakePoint(live.width+1,16)));
  }
  LoppiEntryGeometry compact={44,32,32,8,2};
  assert(hitRegion(compact,0,0,NO,NSMakePoint(22,16)));
  assert(!hitRegion(compact,0,0,NO,NSMakePoint(0,0)));
  LoppiEntryGeometry floating={132,230,0,72,170,0,0,8};
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
