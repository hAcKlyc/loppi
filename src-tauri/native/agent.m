#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <ServiceManagement/ServiceManagement.h>

// Called on Tauri's main thread, including applicationWillTerminate. A modal
// run loop keeps accessibility and the acknowledgement button responsive.
void loppi_show_exit_error(const char *message) {
  NSAlert *alert = [[NSAlert alloc] init];
  alert.messageText = @"loppi 后台尚未退出";
  alert.informativeText = [NSString stringWithUTF8String:message] ?: @"请稍后重试";
  alert.alertStyle = NSAlertStyleWarning;
  [alert addButtonWithTitle:@"好"];
  [NSApp activateIgnoringOtherApps:YES];
  [alert runModal];
}

int loppi_agent_status(void) {
  if (@available(macOS 13.0, *)) {
    SMAppService *service = [SMAppService loginItemServiceWithIdentifier:@"app.loppi.core"];
    return (int)service.status;
  }
  return -1;
}

int loppi_agent_register(void) {
  if (@available(macOS 13.0, *)) {
    SMAppService *service = [SMAppService loginItemServiceWithIdentifier:@"app.loppi.core"];
    NSError *error = nil;
    return [service registerAndReturnError:&error] ? 0 : (int)error.code;
  }
  return -1;
}

int loppi_agent_unregister(void) {
  if (@available(macOS 13.0, *)) {
    SMAppService *service = [SMAppService loginItemServiceWithIdentifier:@"app.loppi.core"];
    NSError *error = nil;
    return [service unregisterAndReturnError:&error] ? 0 : (int)error.code;
  }
  return -1;
}
