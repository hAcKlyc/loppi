#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <ImageIO/ImageIO.h>
#import <Vision/Vision.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>

int hear_active_display_rect(int32_t *x, int32_t *y, int32_t *width,
                             int32_t *height) {
  if (!x || !y || !width || !height) return 0;
  CGEventRef event = CGEventCreate(NULL);
  if (!event) return 0;
  CGPoint location = CGEventGetLocation(event);
  CFRelease(event);
  CGDirectDisplayID display = 0;
  uint32_t count = 0;
  if (CGGetDisplaysWithPoint(location, 1, &display, &count) != kCGErrorSuccess ||
      count == 0) {
    display = CGMainDisplayID();
  }
  CGRect bounds = CGDisplayBounds(display);
  if (bounds.size.width <= 0 || bounds.size.height <= 0) return 0;
  *x = (int32_t)floor(bounds.origin.x);
  *y = (int32_t)floor(bounds.origin.y);
  *width = (int32_t)ceil(bounds.size.width);
  *height = (int32_t)ceil(bounds.size.height);
  return 1;
}

int hear_ocr_png(const char *path, char **output) {
  if (!path || !output) return 0;
  *output = NULL;
  @autoreleasepool {
    NSString *name = [NSString stringWithUTF8String:path];
    if (!name) return 0;
    NSURL *url = [NSURL fileURLWithPath:name];
    CGImageSourceRef source = CGImageSourceCreateWithURL((__bridge CFURLRef)url, NULL);
    if (!source) return 0;
    CGImageRef image = CGImageSourceCreateImageAtIndex(source, 0, NULL);
    CFRelease(source);
    if (!image) return 0;
    VNRecognizeTextRequest *request = [[VNRecognizeTextRequest alloc] init];
    request.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
    request.recognitionLanguages = @[ @"zh-Hans", @"en-US" ];
    request.usesLanguageCorrection = YES;
    VNImageRequestHandler *handler =
        [[VNImageRequestHandler alloc] initWithCGImage:image options:@{}];
    NSError *error = nil;
    BOOL completed = [handler performRequests:@[ request ] error:&error];
    CGImageRelease(image);
    if (!completed || error) return 0;
    NSMutableArray<NSString *> *lines = [NSMutableArray array];
    for (VNRecognizedTextObservation *observation in request.results) {
      VNRecognizedText *candidate = [[observation topCandidates:1] firstObject];
      if (candidate.string.length > 0) [lines addObject:candidate.string];
    }
    NSString *joined = [lines componentsJoinedByString:@"\n"];
    *output = strdup(joined.UTF8String ?: "");
    return *output ? 1 : 0;
  }
}
