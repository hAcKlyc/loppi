#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>

// The foreground app owns the interactive first-use request. The login item
// checks the same responsible app's TCC decision without blocking IPC on an
// invisible prompt.
int hear_microphone_permission_status(void) {
  return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}
