// audience: internal
// # cn-local-payment-hook
// 商品奖励由本地 payment/start 发放, 原生付款入口通过 PAYCALLBACK 触发原有订单确认流程.

#import <Foundation/Foundation.h>
#import <objc/runtime.h>

@protocol StarpointCnPaymentCallback <NSObject>
+ (instancetype)instance;
- (void)payCallBack:(NSString *)response;
@end

// //// 识别接收单个对象的原生回调 [@x380kkm 2026-09-22] ////
static BOOL acceptsPaymentResponse(Method method) {
    if (method == NULL || method_getNumberOfArguments(method) != 3) {
        return NO;
    }
    NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
    return signature.methodReturnType[0] == 'v' &&
        [signature getArgumentTypeAtIndex:2][0] == '@';
}

static BOOL returnsPaymentCallback(Method method) {
    if (method == NULL || method_getNumberOfArguments(method) != 2) {
        return NO;
    }
    NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
    return signature.methodReturnType[0] == '@';
}
// //// /识别接收单个对象的原生回调 ////

// //// 在客户端购买调用返回后通知本地订单成功 [@x380kkm 2026-09-22] ////
static void installLocalPaymentCallback(void) {
    Class sdkClass = NSClassFromString(@"LeitingSDK");
    Class<StarpointCnPaymentCallback> callbackClass = NSClassFromString(@"MyNativeLib");
    Method pay = class_getInstanceMethod(sdkClass, NSSelectorFromString(@"pay:"));
    Method callback = class_getInstanceMethod(callbackClass, @selector(payCallBack:));
    Method instance = class_getClassMethod(callbackClass, @selector(instance));
    if (!acceptsPaymentResponse(pay) || !acceptsPaymentResponse(callback) || !returnsPaymentCallback(instance)) {
        return;
    }
    IMP localPay = imp_implementationWithBlock(^(__unused id receiver, __unused NSString *request) {
        dispatch_async(dispatch_get_main_queue(), ^{
            [[callbackClass instance] payCallBack:@"{\"status\":\"1\",\"resultCode\":\"0\",\"resultMsg\":\"\"}"];
        });
    });
    method_setImplementation(pay, localPay);
}
// //// /在客户端购买调用返回后通知本地订单成功 ////

// //// 在原生类注册后连接商品领取入口 [@x380kkm 2026-09-22] ////
__attribute__((constructor)) static void installCnLocalPaymentHook(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        installLocalPaymentCallback();
    });
}
