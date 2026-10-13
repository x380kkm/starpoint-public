// audience: internal
// # cn-local-payment-tests
// 此测试通过真实 Objective-C 消息分发核对 SDK 付款替换和异步成功回调.

#import <Foundation/Foundation.h>
#include <assert.h>
#include <stdio.h>

static NSUInteger originalPayments;
static NSUInteger completedPayments;

@interface LeitingSDK : NSObject
- (void)pay:(NSString *)request;
@end

@implementation LeitingSDK
- (void)pay:(__unused NSString *)request {
    originalPayments += 1;
}
@end

@interface MyNativeLib : NSObject
+ (instancetype)instance;
- (void)payCallBack:(NSString *)response;
@end

@implementation MyNativeLib
+ (instancetype)instance {
    static MyNativeLib *library;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{ library = [MyNativeLib new]; });
    return library;
}
- (void)payCallBack:(NSString *)response {
    NSDictionary *result = [NSJSONSerialization JSONObjectWithData:[response dataUsingEncoding:NSUTF8StringEncoding] options:0 error:NULL];
    assert([result[@"status"] isEqual:@"1"]);
    assert(NSThread.isMainThread);
    completedPayments += 1;
}
@end

#include "../PersonalServiceBootstrap/StarpointCNLocalPaymentHook.m"

// //// 调用付款入口并核对主线程异步通知 [@x380kkm 2026-09-22] ////
int main(void) {
    @autoreleasepool {
        dispatch_async(dispatch_get_main_queue(), ^{
            [[LeitingSDK new] pay:@"{\"orderId\":\"local-order\"}"];
            assert(originalPayments == 0 && completedPayments == 0);
        });
        NSDate *limit = [NSDate dateWithTimeIntervalSinceNow:1];
        while (completedPayments == 0 && limit.timeIntervalSinceNow > 0) {
            [NSRunLoop.currentRunLoop runMode:NSDefaultRunLoopMode beforeDate:limit];
        }
        assert(originalPayments == 0 && completedPayments == 1);
        puts("原生商品入口异步通知成功, SDK 付款方法调用次数为 0.");
    }
    return 0;
}
