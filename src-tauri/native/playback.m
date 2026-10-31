#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

typedef void (*LoppiOutputEnded)(uint64_t, uint64_t, uint64_t, int);
@interface LoppiOutput : NSObject <AVAudioPlayerDelegate>
@property uint64_t identifier;
@property uint64_t epoch;
@property uint64_t nextBuffer;
@property double origin;
@property BOOL active;
@property NSMutableDictionary<NSNumber *, AVAudioPlayer *> *players;
@property AVAudioPlayer *clock;
@property LoppiOutputEnded ended;
- (void)stop;
@end
@implementation LoppiOutput
- (instancetype)init { if ((self=[super init])) _players=[NSMutableDictionary dictionary]; return self; }
- (void)stop {
  self.active=NO;
  for (AVAudioPlayer *player in self.players.allValues) { player.delegate=nil; [player stop]; }
  [self.players removeAllObjects];
  [self.clock stop]; self.clock=nil;
}
- (void)audioPlayerDidFinishPlaying:(AVAudioPlayer *)player successfully:(BOOL)success {
  NSNumber *key=[self.players allKeysForObject:player].firstObject;
  if (!key) return;
  player.delegate=nil;
  [self.players removeObjectForKey:key];
  if (self.ended) self.ended(self.identifier,key.unsignedLongLongValue,self.epoch,success);
}
- (void)audioPlayerDecodeErrorDidOccur:(AVAudioPlayer *)player error:(NSError *)error {
  (void)error; [self audioPlayerDidFinishPlaying:player successfully:NO];
}
@end
static LoppiOutput *output;
static uint64_t nextContext;
static NSDictionary *failure(NSString *code) { return @{ @"ok":@NO,@"error":code }; }
static NSDictionary *success(NSDictionary *data) { return @{ @"ok":@YES,@"data":data ?: @{} }; }

// A silent native output keeps one device clock and process AudioObject alive
// while playing. It stops on pause/close. User audio never enters WebKit's GPU
// process, so the public pre-26 Tap process exclusion can cover all playback.
static NSData *silent_clock_wav(void) {
  uint8_t bytes[44+960*2]={0};
  memcpy(bytes,"RIFF",4); uint32_t riff=sizeof(bytes)-8; memcpy(bytes+4,&riff,4);
  memcpy(bytes+8,"WAVEfmt ",8); uint32_t fmt=16; memcpy(bytes+16,&fmt,4);
  uint16_t pcm=1, channels=1, alignment=2, bits=16; uint32_t rate=48000, byteRate=96000, size=sizeof(bytes)-44;
  memcpy(bytes+20,&pcm,2); memcpy(bytes+22,&channels,2); memcpy(bytes+24,&rate,4);
  memcpy(bytes+28,&byteRate,4); memcpy(bytes+32,&alignment,2); memcpy(bytes+34,&bits,2);
  memcpy(bytes+36,"data",4); memcpy(bytes+40,&size,4);
  return [NSData dataWithBytes:bytes length:sizeof(bytes)];
}
static NSDictionary *command(NSDictionary *request, LoppiOutputEnded ended) {
  NSString *type=request[@"type"];
  if ([type isEqual:@"open"]) {
    for (NSNumber *key in output.players.allKeys)
      if (output.ended) output.ended(output.identifier,key.unsignedLongLongValue,output.epoch,-1);
    [output stop]; output=[LoppiOutput new]; output.identifier=++nextContext; output.ended=ended;
    return success(@{@"context":@(output.identifier)});
  }
  uint64_t context=[request[@"context"] unsignedLongLongValue];
  uint64_t epoch=[request[@"epoch"] unsignedLongLongValue];
  if (!output || context!=output.identifier) return failure(@"playback_stale");
  if (epoch<output.epoch) return failure(@"playback_stale");
  if ([type isEqual:@"close"]) { [output stop]; output=nil; return success(nil); }
  if ([type isEqual:@"suspend"]) { [output stop]; output.epoch=epoch; return success(nil); }
  if ([type isEqual:@"resume"]) {
    [output stop]; output.epoch=epoch;
    output.clock=[[AVAudioPlayer alloc] initWithData:silent_clock_wav() error:nil];
    output.clock.numberOfLoops=-1;
    if (!output.clock || ![output.clock prepareToPlay] || ![output.clock play]) { [output stop]; return failure(@"playback_device"); }
    output.origin=output.clock.deviceCurrentTime;
    output.active=YES;
    return success(@{@"clock":@0});
  }
  if (epoch!=output.epoch || !output.active) return failure(@"playback_stale");
  if ([type isEqual:@"load"]) {
    NSString *encoded=request[@"wav"];
    if (![encoded isKindOfClass:NSString.class] || encoded.length>1500000 || output.players.count>=2) return failure(@"playback_buffer_limit");
    NSData *data=[[NSData alloc] initWithBase64EncodedString:encoded options:0];
    if (!data || data.length>1000000) return failure(@"playback_invalid_audio");
    AVAudioPlayer *player=[[AVAudioPlayer alloc] initWithData:data error:nil];
    if (!player || player.duration<=0 || player.duration>5.001 || player.format.channelCount!=1 || player.format.sampleRate!=48000 || ![player prepareToPlay]) return failure(@"playback_invalid_audio");
    uint64_t identifier=++output.nextBuffer;
    player.delegate=output; output.players[@(identifier)]=player;
    return success(@{@"buffer":@(identifier),@"duration":@(player.duration),@"sampleRate":@(player.format.sampleRate),@"numberOfChannels":@(player.format.channelCount),@"clock":@(MAX(0,output.clock.deviceCurrentTime-output.origin))});
  }
  NSNumber *identifier=request[@"buffer"];
  AVAudioPlayer *player=output.players[identifier];
  if ([type isEqual:@"remove"]) {
    player.delegate=nil; [player stop]; [output.players removeObjectForKey:identifier]; return success(nil);
  }
  if ([type isEqual:@"schedule"]) {
    double when=[request[@"when"] doubleValue];
    if (!player || !isfinite(when) || when<0 || when>86400) return failure(@"playback_invalid_schedule");
    when=MAX(when,output.clock.deviceCurrentTime-output.origin+0.015);
    if (![player playAtTime:output.origin+when]) return failure(@"playback_device");
    return success(@{@"when":@(when)});
  }
  return failure(@"playback_invalid_command");
}
char *loppi_output_command(const char *bytes, size_t count, LoppiOutputEnded ended) {
  @autoreleasepool {
    NSDictionary *result;
    if (!NSThread.isMainThread) result=failure(@"playback_thread");
    else if (!bytes || count>1600000) result=failure(@"playback_invalid_command");
    else {
      id request=[NSJSONSerialization JSONObjectWithData:[NSData dataWithBytes:bytes length:count] options:0 error:nil];
      result=[request isKindOfClass:NSDictionary.class] ? command(request,ended) : failure(@"playback_invalid_command");
    }
    NSData *data=[NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
    return data ? strndup(data.bytes,data.length) : NULL;
  }
}
void loppi_output_shutdown(void) { if (NSThread.isMainThread) { [output stop]; output=nil; } }
