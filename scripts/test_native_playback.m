// Real AVAudioPlayer/device clock with synthetic silence only, no capture/cloud.
#import "../src-tauri/native/playback.m"
static int completed;
static void ended(uint64_t context,uint64_t buffer,uint64_t epoch,int status) {
  (void)context;(void)buffer;(void)epoch;
  if (status==1) completed++;
}
static NSDictionary *perform(NSDictionary *request) {
  NSData *data=[NSJSONSerialization dataWithJSONObject:request options:0 error:nil];
  char *response=loppi_output_command(data.bytes,data.length,ended);
  NSCAssert(response,@"native response");
  NSDictionary *result=[NSJSONSerialization JSONObjectWithData:[NSData dataWithBytes:response length:strlen(response)] options:0 error:nil];
  free(response); return result;
}
int main() {@autoreleasepool {
 NSDictionary *opened=perform(@{@"type":@"open"});
 NSNumber *context=opened[@"data"][@"context"];
 NSCAssert(context,@"opened context");
 NSCAssert([perform(@{@"type":@"resume",@"context":context,@"epoch":@1})[@"ok"] boolValue],@"resume");
 NSString *wav=[silent_clock_wav() base64EncodedStringWithOptions:0];
 NSMutableArray *buffers=[NSMutableArray array];
 for(int i=0;i<2;i++) {
  NSDictionary *loaded=perform(@{@"type":@"load",@"context":context,@"epoch":@1,@"wav":wav});
  NSCAssert([loaded[@"ok"] boolValue],@"prepared buffer");
  NSCAssert(fabs([loaded[@"data"][@"duration"] doubleValue]-.02)<.0001,@"exact duration");
  [buffers addObject:loaded[@"data"][@"buffer"]];
 }
 NSCAssert(![perform(@{@"type":@"load",@"context":context,@"epoch":@1,@"wav":wav})[@"ok"] boolValue],@"bounded two buffers");
 for(int i=0;i<2;i++) NSCAssert([perform(@{@"type":@"schedule",@"context":context,@"epoch":@1,@"buffer":buffers[i],@"when":@(.1+i*.02)})[@"ok"] boolValue],@"scheduled native clock");
 NSDate *deadline=[NSDate dateWithTimeIntervalSinceNow:2];
 while(completed<2 && deadline.timeIntervalSinceNow>0) [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
 NSCAssert(completed==2,@"native completion callbacks");
 NSCAssert([perform(@{@"type":@"suspend",@"context":context,@"epoch":@3})[@"ok"] boolValue],@"pause");
 NSCAssert(![perform(@{@"type":@"resume",@"context":context,@"epoch":@2})[@"ok"] boolValue],@"late resume rejected");
 NSNumber *newContext=perform(@{@"type":@"open"})[@"data"][@"context"];
 NSCAssert(![perform(@{@"type":@"close",@"context":context,@"epoch":@4})[@"ok"] boolValue],@"late close cannot touch new context");
 NSCAssert([perform(@{@"type":@"close",@"context":newContext,@"epoch":@0})[@"ok"] boolValue],@"close current");
 loppi_output_shutdown();
 puts("native playback: bounded buffers, hardware-clock scheduling, completion, pause and stale epochs passed");
 return 0;
}}
