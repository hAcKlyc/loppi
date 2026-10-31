#import <AVFoundation/AVFoundation.h>
#import <CoreAudio/CoreAudio.h>
#import <Foundation/Foundation.h>

// The foreground app owns the interactive first-use request. The login item
// checks the same responsible app's TCC decision without blocking IPC on an
// invisible prompt.
int loppi_microphone_permission_status(void) {
  return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}

// Recording should follow the machine's normal idle-sleep policy.
int loppi_allow_audio_idle_sleep(void) {
  AudioObjectPropertyAddress address = {
      kAudioHardwarePropertySleepingIsAllowed,
      kAudioObjectPropertyScopeGlobal,
      kAudioObjectPropertyElementMain,
  };
  UInt32 allowed = 1;
  return (int)AudioObjectSetPropertyData(kAudioObjectSystemObject, &address, 0,
                                         NULL, sizeof(allowed), &allowed);
}
