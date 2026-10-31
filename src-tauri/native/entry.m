#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#import <CoreGraphics/CoreGraphics.h>
#import <QuartzCore/QuartzCore.h>
#include <stdint.h>

typedef void (*HearEntryEvent)(int, int, uint64_t);
typedef struct { double width,height,band,rabbit_x,rabbit_y,top_band,top_shift,menu_y; } HearEntryGeometry;
static const void *HearEntryKey=&HearEntryKey;

// Only user presentation changes animate. System/layout changes settle now.
static double motionDuration(NSInteger oldMode, NSInteger oldView, NSInteger mode, NSInteger view, BOOL visible, BOOL reducedMotion) {
  if(oldMode!=0 || mode!=0 || !visible || reducedMotion) return 0;
  if(oldView==0 && view==1) return 0.30;
  if(oldView!=0 && view==0) return 0.14;
  return 0;
}

static NSRect expansionPeak(NSRect current, NSRect target) {
  NSRect peak=target;
  peak.size.width+=(target.size.width-current.size.width)*0.018;
  peak.size.height+=(target.size.height-current.size.height)*0.018;
  peak.origin.x+=(target.origin.x-current.origin.x)*0.018;
  peak.origin.y=NSMaxY(target)-peak.size.height;
  return peak;
}

static HearEntryGeometry topLayout(NSRect screen, double safeTop, NSRect left, NSRect right, NSInteger view, BOOL feedback, BOOL detail, NSRect *frame) {
  BOOL notch=safeTop>0 && !NSIsEmptyRect(left) && !NSIsEmptyRect(right) && NSMaxX(left)<NSMinX(right);
  double center=notch ? (NSMaxX(left)+NSMinX(right))/2 : NSMidX(screen);
  double rabbitCenter=notch ? NSMinX(right)+22 : center;
  // The compact trigger needs room even on a plain display. Expanded content
  // reserves only a real hardware obstruction, not a fictitious 32pt notch.
  double band=view==0 ? MAX(32,notch ? safeTop : 0) : (notch ? safeTop : 0);
  HearEntryGeometry g={0}; g.band=band; g.top_band=notch ? safeTop : 0;
  g.top_shift=rabbitCenter-center+206-(feedback ? 140 : 22);
  if(view==0) {
    g.width=feedback ? 280 : 44; g.height=band+(feedback ? 48 : 0);
    g.rabbit_x=g.width/2-14; g.rabbit_y=(band-28)/2;
    *frame=NSMakeRect(rabbitCenter-g.width/2,NSMaxY(screen)-g.height,g.width,g.height);
  } else {
    g.width=412; g.height=band+(view==2 ? 152 : 128)+(detail && view==1 ? 40 : 0);
    g.rabbit_x=20; g.rabbit_y=band+24;
    *frame=NSMakeRect(center-g.width/2,NSMaxY(screen)-g.height,g.width,g.height);
  }
  return g;
}

static HearEntryGeometry hitGeometry(HearEntryGeometry g, NSInteger mode, NSInteger view, NSRect frame) {
  // NSWindow.frame follows the actual AppKit resize, not the final layout.
  if(mode==0 && view!=0) { g.width=frame.size.width; g.height=frame.size.height; }
  return g;
}

static NSPoint floatingAnchor(NSRect visible, NSPoint point, BOOL snap) {
  double left=NSMinX(visible)+34, right=MAX(left,NSMaxX(visible)-34);
  double bottom=NSMinY(visible)+34, top=MAX(bottom,NSMaxY(visible)-34);
  point.x=MIN(MAX(point.x,left),right); point.y=MIN(MAX(point.y,bottom),top);
  if(snap) {
    double distances[]={point.x-left,right-point.x,point.y-bottom,top-point.y};
    int edge=0;
    for(int i=1;i<4;i++) if(distances[i]<distances[edge]) edge=i;
    if(edge==0) point.x=left; else if(edge==1) point.x=right;
    else if(edge==2) point.y=bottom; else point.y=top;
  }
  return point;
}

static HearEntryGeometry floatingLayout(NSRect visible, NSPoint anchor, BOOL expanded, NSRect *frame) {
  HearEntryGeometry g={0};
  g.width=expanded ? 132 : 68; g.height=expanded ? 230 : 68;
  BOOL down=NSMaxY(visible)-anchor.y<196;
  double x=anchor.x-(expanded && anchor.x>NSMidX(visible) ? 98 : 34);
  double y=anchor.y-(expanded && down ? 196 : 34);
  if(expanded) {
    x=MIN(MAX(x,NSMinX(visible)),NSMaxX(visible)-g.width);
    y=MIN(MAX(y,NSMinY(visible)),NSMaxY(visible)-g.height);
  }
  *frame=NSMakeRect(x,y,g.width,g.height);
  g.rabbit_x=anchor.x-x-26; g.rabbit_y=NSMaxY(*frame)-anchor.y-26;
  g.menu_y=down ? 55 : 8;
  return g;
}

static BOOL roundedRegion(NSRect r, double radius, NSPoint p) {
  if(!NSPointInRect(p,r)) return NO;
  double cx=MIN(MAX(p.x,NSMinX(r)+radius),NSMaxX(r)-radius);
  double cy=MIN(MAX(p.y,NSMinY(r)+radius),NSMaxY(r)-radius);
  return pow(p.x-cx,2)+pow(p.y-cy,2)<=radius*radius;
}
static BOOL hitRegion(HearEntryGeometry g, NSInteger mode, NSInteger view, BOOL detail, NSPoint p) {
  double size=mode==0 ? (view==0 ? 28 : 72) : 52;
  BOOL rabbit=pow(p.x-(g.rabbit_x+size/2),2)+pow(p.y-(g.rabbit_y+size/2),2)<=pow(size/2+5,2);
  if(mode==0 && view!=0) {
    // The original rabbit sits in this band. Keep that point inside after
    // expansion so a stationary pointer does not repeatedly close and reopen.
    if(p.x<0 || p.x>g.width || p.y<0 || p.y>g.height) return NO;
    double radius=MIN(26,g.width/2);
    if(p.y>g.height-radius && (p.x<radius || p.x>g.width-radius)) {
      double cx=p.x<radius ? radius : g.width-radius;
      return pow(p.x-cx,2)+pow(p.y-(g.height-radius),2)<=radius*radius;
    }
    return YES;
  }
  if(mode==1 && view!=0) {
    if(roundedRegion(NSMakeRect(8,g.menu_y,116,167),20,p)) return YES;
    double detailX=g.rabbit_x<34 ? g.width-31 : 8;
    if(detail && roundedRegion(NSMakeRect(detailX,g.rabbit_y+19,23,23),8,p)) return YES;
  }
  return rabbit;
}

@interface HearEntry : NSObject
@property (weak) NSWindow *window;
@property HearEntryEvent callback;
@property NSMutableArray *tokens;
@property id globalMonitor;
@property id localMonitor;
@property NSInteger mode;
@property NSInteger view;
@property BOOL feedback;
@property BOOL detail;
@property BOOL inside;
@property BOOL commentFocus;
@property BOOL focusConfirmed;
@property uint64_t focusGeneration;
@property uint64_t layoutGeneration;
@property uint64_t layoutSerial;
@property BOOL drag;
@property BOOL dragPending;
@property NSPoint dragOrigin;
@property NSPoint dragAnchor;
@property BOOL inhibited;
@property (weak) NSWindow *previousWindow;
@property NSPoint anchor;
@property NSString *displayUUID;
@property NSRunningApplication *previous;
@property HearEntryGeometry geometry;
@property NSRect targetFrame;
- (HearEntryGeometry)layout;
- (HearEntryGeometry)layoutAnimated:(double)duration;
- (BOOL)updatePointer:(NSEvent *)event at:(NSPoint)point;
- (BOOL)confirmFocus;
- (BOOL)ownsKeyboard;
- (void)finishLayout:(uint64_t)serial generation:(uint64_t)generation;
@end

@implementation HearEntry
- (instancetype)init { if((self=[super init])) _tokens=[NSMutableArray array]; return self; }
- (void)notify:(int)kind value:(int)value { if(_callback) _callback(kind,value,_layoutGeneration); }
- (BOOL)ownsKeyboard { return NSApp.isActive && _window.isKeyWindow; }
- (BOOL)confirmFocus {
  BOOL confirmed=_commentFocus && _view==2 && [self ownsKeyboard];
  if(confirmed && !_focusConfirmed) {
    _focusConfirmed=YES;
    if(_callback) _callback(9,1,_focusGeneration);
  }
  return confirmed;
}
- (void)finishLayout:(uint64_t)serial generation:(uint64_t)generation {
  if(serial==_layoutSerial && generation==_layoutGeneration && _mode==0 && _view==1)
    [self notify:8 value:1];
}
- (NSString *)uuid:(NSScreen *)screen {
  CGDirectDisplayID did=[screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
  CFUUIDRef uuid=CGDisplayCreateUUIDFromDisplayID(did);
  NSString *value=uuid ? CFBridgingRelease(CFUUIDCreateString(NULL,uuid)) : @"";
  if(uuid) CFRelease(uuid); return value;
}
- (NSScreen *)floatingScreen {
  for(NSScreen *s in NSScreen.screens) if([[self uuid:s] isEqualToString:_displayUUID]) return s;
  return NSScreen.screens.firstObject;
}
- (BOOL)hit:(NSPoint)point {
  NSRect f=_mode==0 && _view==0 ? _targetFrame : _window.frame;
  NSPoint p=NSMakePoint(point.x-f.origin.x,NSMaxY(f)-point.y);
  return hitRegion(hitGeometry(_geometry,_mode,_view,f),_mode,_view,_detail,p);
}
- (BOOL)hitRabbit:(NSPoint)point {
  NSRect f=_window.frame;
  NSPoint p=NSMakePoint(point.x-f.origin.x,NSMaxY(f)-point.y);
  return hitRegion(_geometry,1,0,NO,p);
}
- (BOOL)checkPointer:(NSEvent *)event {
  NSPoint point=event.window ? [event.window convertPointToScreen:event.locationInWindow] : NSEvent.mouseLocation;
  return [self updatePointer:event at:point];
}
- (void)moveFloatingTo:(NSPoint)anchor {
  _anchor=anchor;
  for(NSScreen *s in NSScreen.screens) if(NSPointInRect(anchor,s.frame)) { _displayUUID=[self uuid:s]; break; }
  [self layout];
}
- (void)finishDrag {
  _dragPending=NO; _drag=NO;
  _anchor=floatingAnchor([self floatingScreen].visibleFrame,_anchor,YES);
  [self layout]; [self notify:5 value:0];
}
- (BOOL)updatePointer:(NSEvent *)event at:(NSPoint)point {
  // One native owner handles the entire physical gesture, including release
  // outside the WebView. Do not start AppKit dragging after asynchronous IPC.
  if(_mode==1 && event.type==NSEventTypeLeftMouseDown && event.window==_window && [self hitRabbit:point]) {
    _dragPending=YES; _dragOrigin=point; _dragAnchor=_anchor;
    _window.ignoresMouseEvents=NO;
    return YES;
  }
  if(_dragPending) {
    if(event.type==NSEventTypeLeftMouseDragged || event.type==NSEventTypeLeftMouseUp) {
      double dx=point.x-_dragOrigin.x, dy=point.y-_dragOrigin.y;
      BOOL started=!_drag && hypot(dx,dy)>=7;
      if(started) {
        _drag=YES; _view=0; _inhibited=YES; _inside=NO;
      }
      if(_drag) [self moveFloatingTo:NSMakePoint(_dragAnchor.x+dx,_dragAnchor.y+dy)];
      // Publish the compact geometry, including when a feedback bubble was
      // already visible. The final mouse-up sample also participates in the
      // threshold: a fast/coalesced drag must never become a mark.
      if(started) [self notify:5 value:1];
      if(event.type==NSEventTypeLeftMouseUp) {
        if(_drag) [self finishDrag];
        else { _dragPending=NO; if([self hitRabbit:point]) [self notify:10 value:0]; }
      }
      return YES;
    }
    if(!event) return NO;
  }
  BOOL hit=[self hit:point];
  // Only suppress a pointer still over the current entry. The old expanded
  // panel (and the feedback bubble) must not become an invisible dead zone.
  // An intentional click also reopens without requiring a mouse round trip.
  if(_inhibited && (!hit || (event && event.type==NSEventTypeLeftMouseDown))) { _inhibited=NO; [self notify:7 value:0]; }
  _window.ignoresMouseEvents=!hit;
  if(_inhibited) hit=NO;
  if(hit!=_inside) { _inside=hit; [self notify:1 value:hit]; }
  // Cancel the user intent now, but let entry_layout own the presentation
  // transition so this outside click still has an expanded -> compact origin.
  if(event && event.type==NSEventTypeLeftMouseDown && !hit && _view!=0) { [self releaseFocus:NO]; [self notify:2 value:0]; }
  return NO;
}
- (HearEntryGeometry)layout {
  return [self layoutAnimated:0];
}
- (HearEntryGeometry)layoutAnimated:(double)duration {
  NSScreen *s=_mode==0 ? NSScreen.screens.firstObject : [self floatingScreen];
  if(!s) return _geometry;
  NSRect screen=s.frame;
  HearEntryGeometry g={0};
  NSRect frame;
  if(_mode==0) {
    g=topLayout(screen,s.safeAreaInsets.top,s.auxiliaryTopLeftArea,s.auxiliaryTopRightArea,_view,_feedback,_detail,&frame);
    _window.level=NSStatusWindowLevel;
  } else {
    NSRect visible=s.visibleFrame;
    if(!_displayUUID && !_drag) _anchor=NSMakePoint(NSMaxX(visible)-54,NSMinY(visible)+54);
    if(!_drag) _anchor=floatingAnchor(visible,_anchor,YES);
    g=floatingLayout(visible,_anchor,!_drag && (_view!=0 || _feedback),&frame);
    _displayUUID=[self uuid:s]; _window.level=NSFloatingWindowLevel;
  }
  _geometry=g; _targetFrame=frame;
  uint64_t serial=++_layoutSerial, generation=_layoutGeneration;
  __weak HearEntry *weak=self;
  // NSWindow's animator clamps an overshooting timing curve. Use two native
  // segments instead: a small geometric overshoot, then a short settle. Both
  // are canceled by the same layout serial; there is no second UI clock.
  BOOL opening=duration>0.2;
  NSRect peak=opening ? expansionPeak(_window.frame,frame) : frame;
  [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
    context.duration=opening ? duration-0.10 : duration;
    context.timingFunction=opening
      ? [CAMediaTimingFunction functionWithControlPoints:0.16 :1 :0.3 :1]
      : [CAMediaTimingFunction functionWithControlPoints:0.4 :0 :0.7 :1];
    [[self.window animator] setFrame:peak display:YES];
  } completionHandler:^{
    if(!opening) { [weak finishLayout:serial generation:generation]; return; }
    HearEntry *owner=weak;
    if(!owner || serial!=owner.layoutSerial || generation!=owner.layoutGeneration || owner.mode!=0 || owner.view!=1) return;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
      context.duration=0.10;
      context.timingFunction=[CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
      [[owner.window animator] setFrame:frame display:YES];
    } completionHandler:^{ [weak finishLayout:serial generation:generation]; }];
  }];
  if(_window.isVisible) [_window orderFrontRegardless];
  [self checkPointer:nil];
  return g;
}
- (void)install {
  __weak HearEntry *weak=self;
  NSEventMask mask=NSEventMaskMouseMoved|NSEventMaskLeftMouseDown|NSEventMaskLeftMouseDragged|NSEventMaskLeftMouseUp|NSEventMaskRightMouseDown;
  _globalMonitor=[NSEvent addGlobalMonitorForEventsMatchingMask:mask handler:^(NSEvent *e){ [weak checkPointer:e]; }];
  _localMonitor=[NSEvent addLocalMonitorForEventsMatchingMask:mask handler:^NSEvent *(NSEvent *e){ return [weak checkPointer:e] ? nil : e; }];
  NSNotificationCenter *center=NSNotificationCenter.defaultCenter;
  [_tokens addObject:[center addObserverForName:NSWindowDidBecomeKeyNotification object:_window queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n; [weak confirmFocus];
  }]];
  [_tokens addObject:[center addObserverForName:NSWindowDidResizeNotification object:_window queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    if(weak.mode==0 && weak.view==1) [weak checkPointer:nil];
  }]];
  [_tokens addObject:[center addObserverForName:NSApplicationDidChangeScreenParametersNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    if(weak.drag) [weak finishDrag];
    weak.dragPending=NO;
    weak.view=0; [weak releaseFocus:NO]; [weak layout]; [weak notify:3 value:0];
  }]];
  [_tokens addObject:[NSWorkspace.sharedWorkspace.notificationCenter addObserverForName:NSWorkspaceActiveSpaceDidChangeNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    if(weak.drag) [weak finishDrag];
    weak.dragPending=NO;
    weak.view=0; [weak releaseFocus:NO]; [weak layout]; [weak notify:3 value:0];
  }]];
  for(NSString *name in @[NSApplicationDidBecomeActiveNotification,NSApplicationDidResignActiveNotification]) {
    [_tokens addObject:[center addObserverForName:name object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){ (void)n;
      if(!NSApp.isActive && weak.view==2) {
        weak.inhibited=YES;
        weak.view=0; [weak releaseFocus:NO]; [weak layout]; [weak notify:2 value:0];
      }
      NSWindowCollectionBehavior behavior=weak.window.collectionBehavior & ~(NSWindowCollectionBehaviorIgnoresCycle|NSWindowCollectionBehaviorParticipatesInCycle);
      weak.window.collectionBehavior=behavior|(NSApp.isActive ? NSWindowCollectionBehaviorParticipatesInCycle : NSWindowCollectionBehaviorIgnoresCycle);
      weak.window.excludedFromWindowsMenu=!NSApp.isActive;
      if(NSApp.isActive) [NSApp addWindowsItem:weak.window title:weak.window.title filename:NO];
      [weak notify:4 value:NSApp.isActive];
      [weak confirmFocus];
    }]];
  }
  [_tokens addObject:[center addObserverForName:NSWindowDidChangeOcclusionStateNotification object:_window queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){ (void)n; [weak notify:6 value:weak.window.isVisible && (weak.window.occlusionState & NSWindowOcclusionStateVisible)!=0]; }]];
}
- (void)releaseFocus:(BOOL)restore {
  if(!_commentFocus) return;
  BOOL owned=NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier==NSProcessInfo.processInfo.processIdentifier && _window.isKeyWindow;
  _commentFocus=NO;
  _focusConfirmed=NO;
  if(_window.isKeyWindow) [_window resignKeyWindow];
  if(restore && owned && _previousWindow && _previousWindow.isVisible) [_previousWindow makeKeyAndOrderFront:nil];
  if(restore && owned && _previous && !_previous.terminated && _previous.processIdentifier!=NSProcessInfo.processInfo.processIdentifier) {
    if([NSApp respondsToSelector:@selector(yieldActivationToApplication:)]) {
      // Selector availability keeps the Rust-linked macOS 13 binary independent
      // of Clang's newer OS-version availability runtime helper.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
      [NSApp yieldActivationToApplication:_previous];
#pragma clang diagnostic pop
      [_previous activateWithOptions:0];
    } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
      [_previous activateWithOptions:NSApplicationActivateIgnoringOtherApps]; // macOS 13
#pragma clang diagnostic pop
    }
  }
  _previous=nil; _previousWindow=nil;
}
- (void)dispose {
  ++_layoutSerial;
  [self releaseFocus:NO];
  if(_globalMonitor) [NSEvent removeMonitor:_globalMonitor];
  if(_localMonitor) [NSEvent removeMonitor:_localMonitor];
  for(id token in _tokens) { [NSNotificationCenter.defaultCenter removeObserver:token]; [NSWorkspace.sharedWorkspace.notificationCenter removeObserver:token]; }
  [_tokens removeAllObjects];
}
@end

static HearEntry *entry(void *window) { return objc_getAssociatedObject((__bridge NSWindow *)window,HearEntryKey); }
void hear_entry_install(void *window,HearEntryEvent callback) {
  NSWindow *w=(__bridge NSWindow *)window;
  HearEntry *e=[HearEntry new]; e.window=w; e.callback=callback;
  objc_setAssociatedObject(w,HearEntryKey,e,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  w.collectionBehavior=NSWindowCollectionBehaviorCanJoinAllSpaces|NSWindowCollectionBehaviorFullScreenAuxiliary|NSWindowCollectionBehaviorCanJoinAllApplications|NSWindowCollectionBehaviorIgnoresCycle;
  w.hidesOnDeactivate=NO; w.hasShadow=NO; w.backgroundColor=NSColor.clearColor;
  w.acceptsMouseMovedEvents=YES;
  w.excludedFromWindowsMenu=!NSApp.isActive;
  if(NSApp.isActive) { w.collectionBehavior=(w.collectionBehavior & ~NSWindowCollectionBehaviorIgnoresCycle)|NSWindowCollectionBehaviorParticipatesInCycle; [NSApp addWindowsItem:w title:w.title filename:NO]; }
  [e install]; [e notify:4 value:NSApp.isActive];
}
HearEntryGeometry hear_entry_layout(void *window,int mode,int view,int feedback,int detail,int suppress,uint64_t generation) {
  HearEntry *e=entry(window);
  if(e.mode!=mode) { if(e.drag) [e finishDrag]; e.dragPending=NO; }
  if(e.drag) view=0;
  e.layoutGeneration=generation;
  double duration=motionDuration(e.mode,e.view,mode,view,e.window.isVisible,NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion);
  // Arm only for an actual dismissal, never for a feedback-only resize.
  if(view!=0) {
    if(e.inhibited) { e.inhibited=NO; [e notify:7 value:0]; }
  } else if(suppress && e.view!=0) e.inhibited=YES;
  e.mode=mode; e.view=view; e.feedback=feedback; e.detail=detail;
  HearEntryGeometry geometry=[e layoutAnimated:duration];
  if(suppress && !e.inhibited) [e notify:7 value:0];
  return geometry;
}
int hear_entry_focus(void *window,int acquire,int restore,uint64_t generation) {
  HearEntry *e=entry(window);
  if(acquire) {
    if(e.view!=2) return 0;
    if(e.commentFocus) return [e confirmFocus];
    if(!e.commentFocus) { e.previous=NSWorkspace.sharedWorkspace.frontmostApplication; e.previousWindow=NSApp.isActive ? NSApp.keyWindow : nil; }
    e.commentFocus=YES; e.focusConfirmed=NO; e.focusGeneration=generation;
    if([NSApp respondsToSelector:@selector(activate)]) [NSApp activate];
    else [NSApp activateIgnoringOtherApps:YES]; // macOS 13
    [e.window makeKeyAndOrderFront:nil];
    return [e confirmFocus]; // A request is not proof of activation.
  } else [e releaseFocus:restore!=0];
  return 1;
}
char *hear_entry_position(void *window) {
  HearEntry *e=entry(window); NSScreen *s=[e floatingScreen]; NSRect f=s.visibleFrame;
  NSDictionary *p=@{@"display_uuid":e.displayUUID ?: @"", @"x":@((e.anchor.x-NSMinX(f))/f.size.width), @"y":@((e.anchor.y-NSMinY(f))/f.size.height)};
  NSData *data=[NSJSONSerialization dataWithJSONObject:p options:0 error:nil];
  return strdup([[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding].UTF8String);
}
void hear_entry_restore(void *window,const char *uuid,double x,double y) {
  HearEntry *e=entry(window); e.displayUUID=[NSString stringWithUTF8String:uuid];
  NSScreen *s=[e floatingScreen]; NSRect f=s.visibleFrame;
  e.anchor=NSMakePoint(NSMinX(f)+x*f.size.width,NSMinY(f)+y*f.size.height);
}
uint32_t hear_entry_window_id(void *window) { return (uint32_t)((__bridge NSWindow *)window).windowNumber; }
void hear_entry_dispose(void *window) {
  HearEntry *e=entry(window);
  [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
    context.duration=0;
    [[e.window animator] setFrame:e.window.frame display:NO];
  } completionHandler:nil];
  [e dispose]; objc_setAssociatedObject((__bridge NSWindow *)window,HearEntryKey,nil,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

int hear_entry_active(void) { return NSApp.isActive ? 1 : 0; }

HearEntryGeometry hear_entry_geometry(void *window) { return entry(window).geometry; }
