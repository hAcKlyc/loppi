#import <Foundation/Foundation.h>
#import <ServiceManagement/ServiceManagement.h>

int hear_agent_status(void) {
  if (@available(macOS 13.0, *)) {
    SMAppService *service = [SMAppService loginItemServiceWithIdentifier:@"app.hear.core"];
    return (int)service.status;
  }
  return -1;
}

int hear_agent_register(void) {
  if (@available(macOS 13.0, *)) {
    SMAppService *service = [SMAppService loginItemServiceWithIdentifier:@"app.hear.core"];
    NSError *error = nil;
    return [service registerAndReturnError:&error] ? 0 : (int)error.code;
  }
  return -1;
}

int hear_agent_unregister(void) {
  if (@available(macOS 13.0, *)) {
    SMAppService *service = [SMAppService loginItemServiceWithIdentifier:@"app.hear.core"];
    NSError *error = nil;
    return [service unregisterAndReturnError:&error] ? 0 : (int)error.code;
  }
  return -1;
}
