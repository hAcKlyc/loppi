#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdint.h>

typedef void (*HearEntryEvent)(int, int);
typedef struct { double width,height,band,rabbit_x,rabbit_y; } HearEntryGeometry;
static const void *HearEntryKey=&HearEntryKey;

static BOOL unfolds(NSInteger oldMode, NSInteger oldView, NSInteger mode, NSInteger view, BOOL visible, BOOL reducedMotion) {
  return oldMode==0 && mode==0 && oldView==0 && view==1 && visible && !reducedMotion;
}

static HearEntryGeometry hitGeometry(HearEntryGeometry g, NSInteger mode, NSInteger view, NSRect frame) {
  // NSWindow.frame follows the actual AppKit resize, not the final layout.
  if(mode==0 && view!=0) { g.width=frame.size.width; g.height=frame.size.height; }
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
    if(roundedRegion(NSMakeRect(8,8,116,167),20,p)) return YES;
    if(detail && roundedRegion(NSMakeRect(8,g.height-41,23,23),8,p)) return YES;
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
@property BOOL drag;
@property BOOL inhibited;
@property NSRect suppressedFrame;
@property (weak) NSWindow *previousWindow;
@property NSPoint anchor;
@property NSString *displayUUID;
@property NSRunningApplication *previous;
@property HearEntryGeometry geometry;
- (HearEntryGeometry)layout;
- (HearEntryGeometry)layoutAnimated:(BOOL)animated;
@end

@implementation HearEntry
- (instancetype)init { if((self=[super init])) _tokens=[NSMutableArray array]; return self; }
- (void)notify:(int)kind value:(int)value { if(_callback) _callback(kind,value); }
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
  NSRect f=_window.frame;
  NSPoint p=NSMakePoint(point.x-f.origin.x,NSMaxY(f)-point.y);
  return hitRegion(hitGeometry(_geometry,_mode,_view,f),_mode,_view,_detail,p);
}
- (void)checkPointer:(NSEvent *)event {
  if(_drag) {
    if(event && event.type==NSEventTypeLeftMouseUp) { _drag=NO; [self layout]; _callback(5,0); }
    else return;
  }
  if(_inhibited && event && !NSPointInRect(NSEvent.mouseLocation,_suppressedFrame)) { _inhibited=NO; _callback(7,0); }
  BOOL hit=[self hit:NSEvent.mouseLocation];
  _window.ignoresMouseEvents=!hit;
  if(_inhibited) hit=NO;
  if(hit!=_inside) { _inside=hit; _callback(1,hit); }
  if(event && event.type==NSEventTypeLeftMouseDown && !hit && _view!=0) { _view=0; [self releaseFocus:NO]; _callback(2,0); }
}
- (HearEntryGeometry)layout {
  return [self layoutAnimated:NO];
}
- (HearEntryGeometry)layoutAnimated:(BOOL)animated {
  NSScreen *s=_mode==0 ? NSScreen.screens.firstObject : [self floatingScreen];
  if(!s) return _geometry;
  NSRect screen=s.frame;
  HearEntryGeometry g={0};
  NSRect frame;
  if(_mode==0) {
    double band=MAX(32,s.safeAreaInsets.top);
    BOOL notch=s.safeAreaInsets.top>0 && !NSIsEmptyRect(s.auxiliaryTopRightArea);
    double center=notch ? (NSMaxX(s.auxiliaryTopLeftArea)+NSMinX(s.auxiliaryTopRightArea))/2 : NSMidX(screen);
    double rabbitCenter=notch ? NSMinX(s.auxiliaryTopRightArea)+22 : center;
    g.band=band;
    if(_view==0) {
      g.width=_feedback ? 280 : 44; g.height=band+(_feedback ? 48 : 0);
      g.rabbit_x=g.width/2-14; g.rabbit_y=(band-28)/2;
      frame=NSMakeRect(rabbitCenter-g.width/2,NSMaxY(screen)-g.height,g.width,g.height);
    } else {
      g.width=412; g.height=band+(_view==2 ? 152 : 128)+(_detail && _view==1 ? 40 : 0);
      g.rabbit_x=20; g.rabbit_y=band+24;
      frame=NSMakeRect(center-g.width/2,NSMaxY(screen)-g.height,g.width,g.height);
    }
    _window.level=NSStatusWindowLevel;
  } else {
    NSRect visible=s.visibleFrame;
    if(NSEqualPoints(_anchor,NSZeroPoint)) _anchor=NSMakePoint(NSMaxX(visible)-54,NSMinY(visible)+54);
    // Reserve the full upwards menu even while collapsed.
    _anchor.x=MIN(MAX(_anchor.x,NSMinX(visible)+98),NSMaxX(visible)-34);
    _anchor.y=MIN(MAX(_anchor.y,NSMinY(visible)+34),NSMaxY(visible)-196);
    g.width=(_view!=0 || _feedback) ? 132 : 68;
    g.height=(_view!=0 || _feedback) ? 230 : 68;
    g.rabbit_x=g.width-60; g.rabbit_y=g.height-60;
    frame=NSMakeRect(_anchor.x-(g.width-34),_anchor.y-34,g.width,g.height);
    _displayUUID=[self uuid:s]; _window.level=NSFloatingWindowLevel;
  }
  _geometry=g;
  // The animator is asynchronous. A zero-duration update also cancels an
  // in-flight resize, so actions, input focus and screen changes never wait.
  [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
    context.duration=animated ? 0.22 : 0;
    [[self.window animator] setFrame:frame display:YES];
  } completionHandler:nil];
  if(_window.isVisible) [_window orderFrontRegardless];
  [self checkPointer:nil];
  return g;
}
- (void)install {
  __weak HearEntry *weak=self;
  NSEventMask mask=NSEventMaskMouseMoved|NSEventMaskLeftMouseDown|NSEventMaskLeftMouseDragged|NSEventMaskLeftMouseUp|NSEventMaskRightMouseDown;
  _globalMonitor=[NSEvent addGlobalMonitorForEventsMatchingMask:mask handler:^(NSEvent *e){ [weak checkPointer:e]; }];
  _localMonitor=[NSEvent addLocalMonitorForEventsMatchingMask:mask handler:^NSEvent *(NSEvent *e){ [weak checkPointer:e]; return e; }];
  NSNotificationCenter *center=NSNotificationCenter.defaultCenter;
  [_tokens addObject:[center addObserverForName:NSWindowDidResizeNotification object:_window queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    if(weak.mode==0 && weak.view==1) [weak checkPointer:nil];
  }]];
  [_tokens addObject:[center addObserverForName:NSApplicationDidChangeScreenParametersNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    weak.view=0; [weak releaseFocus:NO]; [weak layout]; [weak notify:3 value:0];
  }]];
  [_tokens addObject:[NSWorkspace.sharedWorkspace.notificationCenter addObserverForName:NSWorkspaceActiveSpaceDidChangeNotification object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    weak.view=0; [weak releaseFocus:NO]; [weak layout]; [weak notify:3 value:0];
  }]];
  for(NSString *name in @[NSApplicationDidBecomeActiveNotification,NSApplicationDidResignActiveNotification]) {
    [_tokens addObject:[center addObserverForName:name object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){ (void)n;
      if(!NSApp.isActive && weak.view==2) {
        weak.suppressedFrame=weak.window.frame;
        weak.inhibited=NSPointInRect(NSEvent.mouseLocation,weak.suppressedFrame);
        weak.view=0; [weak releaseFocus:NO]; [weak layout]; [weak notify:2 value:0];
      }
      NSWindowCollectionBehavior behavior=weak.window.collectionBehavior & ~(NSWindowCollectionBehaviorIgnoresCycle|NSWindowCollectionBehaviorParticipatesInCycle);
      weak.window.collectionBehavior=behavior|(NSApp.isActive ? NSWindowCollectionBehaviorParticipatesInCycle : NSWindowCollectionBehaviorIgnoresCycle);
      weak.window.excludedFromWindowsMenu=!NSApp.isActive;
      if(NSApp.isActive) [NSApp addWindowsItem:weak.window title:weak.window.title filename:NO];
      [weak notify:4 value:NSApp.isActive];
    }]];
  }
  [_tokens addObject:[center addObserverForName:NSWindowDidChangeOcclusionStateNotification object:_window queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){ (void)n; [weak notify:6 value:weak.window.isVisible && (weak.window.occlusionState & NSWindowOcclusionStateVisible)!=0]; }]];
  [_tokens addObject:[center addObserverForName:NSWindowDidMoveNotification object:_window queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *n){
    (void)n;
    if(!weak.drag) return;
    NSRect f=weak.window.frame;
    weak.anchor=NSMakePoint(NSMaxX(f)-34,NSMinY(f)+34);
    for(NSScreen *s in NSScreen.screens) if(NSPointInRect(weak.anchor,s.frame)) weak.displayUUID=[weak uuid:s];
  }]];
}
- (void)releaseFocus:(BOOL)restore {
  if(!_commentFocus) return;
  BOOL owned=NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier==NSProcessInfo.processInfo.processIdentifier && _window.isKeyWindow;
  _commentFocus=NO;
  if(_window.isKeyWindow) [_window resignKeyWindow];
  if(restore && owned && _previousWindow && _previousWindow.isVisible) [_previousWindow makeKeyAndOrderFront:nil];
  if(restore && owned && _previous && !_previous.terminated && _previous.processIdentifier!=NSProcessInfo.processInfo.processIdentifier) [_previous activateWithOptions:NSApplicationActivateIgnoringOtherApps];
  _previous=nil; _previousWindow=nil;
}
- (void)dispose {
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
  [e install]; callback(4,NSApp.isActive);
}
HearEntryGeometry hear_entry_layout(void *window,int mode,int view,int feedback,int detail,int suppress) {
  HearEntry *e=entry(window);
  BOOL animated=unfolds(e.mode,e.view,mode,view,e.window.isVisible,NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion);
  if(suppress && !e.inhibited) { e.suppressedFrame=e.window.frame; e.inhibited=NSPointInRect(NSEvent.mouseLocation,e.suppressedFrame); if(!e.inhibited) e.callback(7,0); }
  e.mode=mode; e.view=view; e.feedback=feedback; e.detail=detail;
  return [e layoutAnimated:animated];
}
int hear_entry_focus(void *window,int acquire,int restore) {
  HearEntry *e=entry(window);
  if(acquire) {
    if(e.view!=2) return 0;
    if(e.commentFocus) return NSApp.isActive && e.window.isKeyWindow;
    if(!e.commentFocus) { e.previous=NSWorkspace.sharedWorkspace.frontmostApplication; e.previousWindow=NSApp.isActive ? NSApp.keyWindow : nil; }
    e.commentFocus=YES; [NSApp activateIgnoringOtherApps:YES]; [e.window makeKeyAndOrderFront:nil];
  } else [e releaseFocus:restore!=0];
  return 1;
}
void hear_entry_drag(void *window,int dragging) { HearEntry *e=entry(window); e.drag=dragging; if(!dragging) [e layout]; }
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
