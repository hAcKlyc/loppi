#import <AVFoundation/AVFoundation.h>
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>

int loppi_microphone_authorization_status(void) {
  return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}

int loppi_request_microphone_authorization(void) {
  AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
  if (status != AVAuthorizationStatusNotDetermined) return (int)status;

  dispatch_semaphore_t completed = dispatch_semaphore_create(0);
  __block BOOL granted = NO;
  [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                           completionHandler:^(BOOL accepted) {
    granted = accepted;
    dispatch_semaphore_signal(completed);
  }];
  if (dispatch_semaphore_wait(completed, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC))) {
    return -1;
  }
  return granted ? AVAuthorizationStatusAuthorized : AVAuthorizationStatusDenied;
}

int loppi_screen_authorization_status(void) {
  return CGPreflightScreenCaptureAccess() ? 1 : 0;
}

int loppi_request_screen_authorization(void) {
  return CGRequestScreenCaptureAccess() ? 1 : 0;
}

int loppi_tap_required(void) {
  NSOperatingSystemVersion version=[[NSProcessInfo processInfo] operatingSystemVersion];
  return version.majorVersion>14 || (version.majorVersion==14 && version.minorVersion>=2);
}

int loppi_microphone_available(void) {
  return [AVCaptureDevice defaultDeviceWithMediaType:AVMediaTypeAudio] != nil;
}
