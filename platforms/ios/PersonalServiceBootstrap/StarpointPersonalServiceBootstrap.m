// audience: internal
// # personal-service-bootstrap
//
// 该 Framework 在载入时启动个人服务. 服务数据位于当前 App 的 Application Support 目录.
// 配置 STARPOINT_CN_CDN_BUNDLE_PATH 或 StarpointCNCDNBundlePath 后, direct 模式直接读取 bundle 内的 CN CDN, import 模式才复制.
// App 返回前台时通过串行队列探测 loopback /health 并恢复监听, 激活收尾等待恢复完成信号.
// 同进程原生宿主通过复制接口读取管理 token.
// TitleScene 控制进程内管理按钮, 按钮在 Safari 中打开仅访问 loopback 的管理页面.
// CN 原生登录请求在 NSURLSession 任务创建时路由到 loopback 服务.

#import <Foundation/Foundation.h>
#import <SafariServices/SafariServices.h>
#import <UIKit/UIKit.h>

#import <objc/runtime.h>

#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdatomic.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#import "starpoint_personal_service.h"
#import "StarpointPersonalServiceBootstrap.h"

@class StarpointManagementEntryTarget;

typedef void (^StarpointPersonalServiceRecoveryCompletion)(BOOL isReady);

static const uint16_t StarpointPersonalServicePort = 17171;
static const int StarpointPersonalServiceHealthProbeTimeoutMilliseconds = 150;
static const NSUInteger StarpointForegroundRecoveryMaximumAttempts = 12;
static const NSUInteger StarpointForegroundRecoveryRestartInterval = 4;
static const uint64_t StarpointForegroundRecoveryRetryDelayNanoseconds =
    250 * NSEC_PER_MSEC;

typedef NS_ENUM(uint8_t, StarpointPersonalServiceHealthState) {
    StarpointPersonalServiceHealthUnreachable,
    StarpointPersonalServiceHealthListening,
    StarpointPersonalServiceHealthReady,
};

static NSString *const StarpointCnCdnBundleEnvironmentKey = @"STARPOINT_CN_CDN_BUNDLE_PATH";
static NSString *const StarpointCnCdnBundleModeEnvironmentKey = @"STARPOINT_CN_CDN_BUNDLE_MODE";
static NSString *const StarpointCnCdnBundleInfoKey = @"StarpointCNCDNBundlePath";
static NSString *const StarpointCnCdnBundleModeInfoKey = @"StarpointCNCDNBundleMode";
static NSString *const StarpointCnCdnBundleModeDirect = @"direct";
static NSString *const StarpointCnCdnImportMarkerName = @".starpoint-bundle-import.plist";
static NSString *const StarpointCnCdnStagingPrefix = @".starpoint-cn-import-";
static StarpointPersonalService *personalService = NULL;
static dispatch_queue_t personalServiceQueue;
static __weak SFSafariViewController *presentedManagementController;
static StarpointManagementEntryTarget *managementEntryTarget;
static UIButton *managementEntryButton;
static _Atomic(uint64_t) backgroundFlushCount = 0;
static _Atomic(int32_t) lastBackgroundFlushResult = -1;
static _Atomic(uint64_t) foregroundResumeCount = 0;
static _Atomic(int32_t) lastStartResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_NOT_ATTEMPTED;
static _Atomic(uint64_t) cnCdnImportCount = 0;
static _Atomic(int32_t) managementEntryVisible = 0;
static _Atomic(uint64_t) foregroundRecoveryScheduled = 0;
static _Atomic(uint64_t) foregroundRecoveryGeneration = 0;
static NSMutableArray *foregroundRecoveryCompletions;
static UIBackgroundTaskIdentifier backgroundExecutionTask = NSUIntegerMax;
static uint64_t backgroundExecutionGeneration = 0;

static void presentManagementPage(void);
static void reconcileManagementEntry(UIApplication *application);
static BOOL ensurePersonalServiceReady(void);
static void schedulePersonalServiceForegroundRecovery(
    StarpointPersonalServiceRecoveryCompletion completion
);

// //// 改写 CN 原生登录请求地址 [@x380kkm 2026-08-25] ////
static BOOL isCnLoginSdkHost(NSString *host) {
    NSString *normalizedHost = host.lowercaseString;
    return [normalizedHost isEqualToString:@"leiting.com"] ||
        [normalizedHost hasSuffix:@".leiting.com"] ||
        [normalizedHost isEqualToString:@"roguelike.com"] ||
        [normalizedHost hasSuffix:@".roguelike.com"] ||
        [normalizedHost isEqualToString:@"cl2009.com"] ||
        [normalizedHost hasSuffix:@".cl2009.com"];
}

static BOOL isFailedLoopbackTlsURL(NSURLComponents *components) {
    NSNumber *port = components.port;
    return [components.scheme.lowercaseString isEqualToString:@"https"] &&
        [components.host.lowercaseString isEqualToString:@"127.0.0.1"] &&
        (port == nil || port.unsignedIntegerValue == 443);
}

static NSURL *routeCnLoginSdkURL(NSURL *url) {
    if (url == nil) {
        return nil;
    }

    NSURLComponents *components = [NSURLComponents componentsWithURL:url
                                             resolvingAgainstBaseURL:NO];
    NSString *scheme = components.scheme.lowercaseString;
    if (![scheme isEqualToString:@"http"] && ![scheme isEqualToString:@"https"]) {
        return url;
    }
    if (!isCnLoginSdkHost(components.host) && !isFailedLoopbackTlsURL(components)) {
        return url;
    }

    components.scheme = @"http";
    components.host = @"127.0.0.1";
    components.port = @(StarpointPersonalServicePort);
    components.user = nil;
    components.password = nil;
    return components.URL ?: url;
}

static NSURLRequest *routeCnLoginSdkRequest(NSURLRequest *request) {
    NSURL *routedURL = routeCnLoginSdkURL(request.URL);
    if (routedURL == nil || [routedURL isEqual:request.URL]) {
        return request;
    }

    NSMutableURLRequest *routedRequest = [request mutableCopy];
    routedRequest.URL = routedURL;
    return routedRequest;
}

@interface NSURLSession (StarpointCnLoginRouting)
- (NSURLSessionDataTask *)starpoint_dataTaskWithRequest:(NSURLRequest *)request;
- (NSURLSessionDataTask *)starpoint_dataTaskWithRequest:(NSURLRequest *)request
                                      completionHandler:(void (^)(NSData *, NSURLResponse *, NSError *))completionHandler;
- (NSURLSessionDataTask *)starpoint_dataTaskWithURL:(NSURL *)url;
- (NSURLSessionDataTask *)starpoint_dataTaskWithURL:(NSURL *)url
                                  completionHandler:(void (^)(NSData *, NSURLResponse *, NSError *))completionHandler;
@end

@implementation NSURLSession (StarpointCnLoginRouting)

- (NSURLSessionDataTask *)starpoint_dataTaskWithRequest:(NSURLRequest *)request {
    return [self starpoint_dataTaskWithRequest:routeCnLoginSdkRequest(request)];
}

- (NSURLSessionDataTask *)starpoint_dataTaskWithRequest:(NSURLRequest *)request
                                      completionHandler:(void (^)(NSData *, NSURLResponse *, NSError *))completionHandler {
    return [self starpoint_dataTaskWithRequest:routeCnLoginSdkRequest(request)
                             completionHandler:completionHandler];
}

- (NSURLSessionDataTask *)starpoint_dataTaskWithURL:(NSURL *)url {
    return [self starpoint_dataTaskWithURL:routeCnLoginSdkURL(url)];
}

- (NSURLSessionDataTask *)starpoint_dataTaskWithURL:(NSURL *)url
                                  completionHandler:(void (^)(NSData *, NSURLResponse *, NSError *))completionHandler {
    return [self starpoint_dataTaskWithURL:routeCnLoginSdkURL(url)
                         completionHandler:completionHandler];
}

@end
// //// /改写 CN 原生登录请求地址 ////

// //// 安装原生登录请求地址改写 [@x380kkm 2026-08-25] ////
static void exchangeURLSessionMethod(SEL originalSelector, SEL routingSelector) {
    Method originalMethod = class_getInstanceMethod(NSURLSession.class, originalSelector);
    Method routingMethod = class_getInstanceMethod(NSURLSession.class, routingSelector);
    if (originalMethod != NULL && routingMethod != NULL) {
        method_exchangeImplementations(originalMethod, routingMethod);
    }
}

static void installNativeLoginURLRouting(void) {
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
      exchangeURLSessionMethod(
          @selector(dataTaskWithRequest:),
          @selector(starpoint_dataTaskWithRequest:)
      );
      exchangeURLSessionMethod(
          @selector(dataTaskWithRequest:completionHandler:),
          @selector(starpoint_dataTaskWithRequest:completionHandler:)
      );
      exchangeURLSessionMethod(
          @selector(dataTaskWithURL:),
          @selector(starpoint_dataTaskWithURL:)
      );
      exchangeURLSessionMethod(
          @selector(dataTaskWithURL:completionHandler:),
          @selector(starpoint_dataTaskWithURL:completionHandler:)
      );
    });
}
// //// /安装原生登录请求地址改写 ////

// //// 管理入口按钮目标 [@x380kkm 2026-08-20] ////
@interface StarpointManagementEntryTarget : NSObject
- (void)openManagementPage;
@end

@implementation StarpointManagementEntryTarget

// //// 从标题页按钮打开管理页面 [@x380kkm 2026-08-20] ////
- (void)openManagementPage {
    presentManagementPage();
}
// //// /从标题页按钮打开管理页面 ////

@end
// //// /管理入口按钮目标 ////

// //// 保留诊断宿主使用的 Framework 链接 [@x380kkm 2026-07-23] ////
void starpoint_personal_service_bootstrap_link(void) {}
// //// /保留诊断宿主使用的 Framework 链接 ////

// //// 向同进程宿主复制管理 token [@x380kkm 2026-07-23] ////
size_t starpoint_personal_service_bootstrap_copy_management_token(
    char *buffer,
    size_t buffer_length
) {
    __block size_t result = 0;
    if (personalServiceQueue == nil) {
        return result;
    }
    dispatch_sync(personalServiceQueue, ^{
      if (personalService != NULL) {
          result = starpoint_personal_service_copy_management_token(
              personalService,
              buffer,
              buffer_length
          );
      }
    });
    return result;
}
// //// /向同进程宿主复制管理 token ////

// //// 返回 iOS 宿主使用的固定 loopback 端口 [@x380kkm 2026-07-24] ////
uint16_t starpoint_personal_service_bootstrap_port(void) {
    return StarpointPersonalServicePort;
}
// //// /返回 iOS 宿主使用的固定 loopback 端口 ////

// //// 返回个人服务运行状态 [@x380kkm 2026-07-24] ////
int32_t starpoint_personal_service_bootstrap_is_running(void) {
    __block int32_t result = 0;
    if (personalServiceQueue == nil) {
        return result;
    }
    dispatch_sync(personalServiceQueue, ^{
      if (personalService != NULL) {
          result = starpoint_personal_service_is_running(personalService);
      }
    });
    return result;
}
// //// /返回个人服务运行状态 ////

// //// 返回真实 App 生命周期处理计数 [@x380kkm 2026-08-18] ////
uint64_t starpoint_personal_service_bootstrap_background_flush_count(void) {
    return atomic_load_explicit(&backgroundFlushCount, memory_order_acquire);
}

int32_t starpoint_personal_service_bootstrap_last_background_flush_result(void) {
    return atomic_load_explicit(&lastBackgroundFlushResult, memory_order_acquire);
}

uint64_t starpoint_personal_service_bootstrap_foreground_resume_count(void) {
    return atomic_load_explicit(&foregroundResumeCount, memory_order_acquire);
}
// //// /返回真实 App 生命周期处理计数 ////

// //// 返回个人服务启动和 CDN 导入状态 [@x380kkm 2026-08-18] ////
StarpointPersonalServiceBootstrapStartResult
starpoint_personal_service_bootstrap_last_start_result(void) {
    return (StarpointPersonalServiceBootstrapStartResult)atomic_load_explicit(
        &lastStartResult,
        memory_order_acquire
    );
}

uint64_t starpoint_personal_service_bootstrap_cdn_import_count(void) {
    return atomic_load_explicit(&cnCdnImportCount, memory_order_acquire);
}
// //// /返回个人服务启动和 CDN 导入状态 ////

// //// 创建当前 App 的个人服务目录 [@x380kkm 2026-07-22] ////
static NSString *createPersonalServiceRoot(void) {
    NSError *error = nil;
    NSURL *applicationSupport = [[NSFileManager defaultManager]
        URLForDirectory:NSApplicationSupportDirectory
               inDomain:NSUserDomainMask
      appropriateForURL:nil
                 create:YES
                  error:&error];
    if (applicationSupport == nil || error != nil) {
        return nil;
    }

    NSURL *serviceRoot = [applicationSupport URLByAppendingPathComponent:@"StarpointPersonalService"
                                                             isDirectory:YES];
    if (![[NSFileManager defaultManager] createDirectoryAtURL:serviceRoot
                                  withIntermediateDirectories:YES
                                                   attributes:nil
                                                        error:&error]) {
        return nil;
    }
    return serviceRoot.path;
}
// //// /创建当前 App 的个人服务目录 ////

typedef NS_ENUM(NSInteger, StarpointCnCdnDirectoryState) {
    StarpointCnCdnDirectoryMissing = 0,
    StarpointCnCdnDirectoryReady = 1,
    StarpointCnCdnDirectoryEmpty = 2,
    StarpointCnCdnDirectoryInvalid = 3,
};

// //// 判断 bundle 相对路径是否位于当前 App bundle 内 [@x380kkm 2026-08-18] ////
static BOOL isDescendantURL(NSURL *candidate, NSURL *root) {
    NSArray<NSString *> *rootComponents = root.pathComponents;
    NSArray<NSString *> *candidateComponents = candidate.pathComponents;
    if (candidateComponents.count <= rootComponents.count) {
        return NO;
    }
    for (NSUInteger index = 0; index < rootComponents.count; index += 1) {
        if (![candidateComponents[index] isEqualToString:rootComponents[index]]) {
            return NO;
        }
    }
    return YES;
}
// //// /判断 bundle 相对路径是否位于当前 App bundle 内 ////

// //// 读取 CN CDN bundle 配置 [@x380kkm 2026-08-18] ////
static NSString *configuredCnCdnBundlePath(BOOL *isConfigured) {
    *isConfigured = NO;
    NSDictionary *environment = NSProcessInfo.processInfo.environment;
    if (environment[StarpointCnCdnBundleEnvironmentKey] != nil) {
        *isConfigured = YES;
        return environment[StarpointCnCdnBundleEnvironmentKey];
    }
    id configuredValue = NSBundle.mainBundle.infoDictionary[StarpointCnCdnBundleInfoKey];
    if (configuredValue != nil) {
        *isConfigured = YES;
        return [configuredValue isKindOfClass:[NSString class]] ? configuredValue : nil;
    }
    return nil;
}
// //// /读取 CN CDN bundle 配置 ////

// //// 读取 CN CDN bundle 使用模式 [@x380kkm 2026-08-18] ////
static NSString *configuredCnCdnBundleMode(void) {
    NSDictionary *environment = NSProcessInfo.processInfo.environment;
    id environmentValue = environment[StarpointCnCdnBundleModeEnvironmentKey];
    if (environmentValue != nil) {
        return [environmentValue isKindOfClass:[NSString class]] ? environmentValue : nil;
    }
    id configuredValue = NSBundle.mainBundle.infoDictionary[StarpointCnCdnBundleModeInfoKey];
    if (configuredValue == nil) {
        return nil;
    }
    return [configuredValue isKindOfClass:[NSString class]] ? configuredValue : nil;
}
// //// /读取 CN CDN bundle 使用模式 ////

// //// 解析并隔离 CN CDN bundle 路径 [@x380kkm 2026-08-18] ////
static NSURL *resolveCnCdnBundleURL(NSString *relativePath) {
    if (relativePath.length == 0 || relativePath.isAbsolutePath) {
        return nil;
    }
    for (NSString *component in relativePath.pathComponents) {
        if ([component isEqualToString:@"."] || [component isEqualToString:@".."] ||
            [component isEqualToString:@"~"]) {
            return nil;
        }
    }
    NSURL *bundleURL = [NSBundle.mainBundle.bundleURL URLByResolvingSymlinksInPath];
    NSURL *candidateURL = [bundleURL URLByAppendingPathComponent:relativePath isDirectory:YES];
    NSURL *resolvedURL = [candidateURL URLByResolvingSymlinksInPath];
    return isDescendantURL(resolvedURL, bundleURL) ? resolvedURL : nil;
}
// //// /解析并隔离 CN CDN bundle 路径 ////

// //// 检查 CN CDN 目录内容并拒绝符号链接 [@x380kkm 2026-08-18] ////
static StarpointCnCdnDirectoryState inspectCnCdnDirectory(
    NSURL *directoryURL,
    NSUInteger *fileCount
) {
    *fileCount = 0;
    NSNumber *isDirectory = nil;
    NSNumber *isSymbolicLink = nil;
    NSError *resourceError = nil;
    if (![directoryURL getResourceValue:&isDirectory
                                 forKey:NSURLIsDirectoryKey
                                  error:&resourceError] || !isDirectory.boolValue) {
        return StarpointCnCdnDirectoryMissing;
    }
    if ([directoryURL getResourceValue:&isSymbolicLink
                                forKey:NSURLIsSymbolicLinkKey
                                 error:&resourceError] && isSymbolicLink.boolValue) {
        return StarpointCnCdnDirectoryInvalid;
    }

    NSArray *keys = @[NSURLIsDirectoryKey, NSURLIsRegularFileKey, NSURLIsSymbolicLinkKey];
    __block BOOL enumerationFailed = NO;
    NSDirectoryEnumerator *enumerator = [NSFileManager.defaultManager
        enumeratorAtURL:directoryURL
        includingPropertiesForKeys:keys
        options:0
        errorHandler:^BOOL(__unused NSURL *url, __unused NSError *error) {
            enumerationFailed = YES;
            return NO;
        }];
    if (enumerator == nil) {
        return StarpointCnCdnDirectoryInvalid;
    }
    for (NSURL *entryURL in enumerator) {
        NSNumber *entryIsSymbolicLink = nil;
        NSNumber *entryIsRegularFile = nil;
        NSError *entryError = nil;
        if (![entryURL getResourceValue:&entryIsSymbolicLink
                                 forKey:NSURLIsSymbolicLinkKey
                                  error:&entryError] || entryIsSymbolicLink.boolValue) {
            return StarpointCnCdnDirectoryInvalid;
        }
        if (![entryURL getResourceValue:&entryIsRegularFile
                                 forKey:NSURLIsRegularFileKey
                                  error:&entryError]) {
            return StarpointCnCdnDirectoryInvalid;
        }
        if (entryIsRegularFile.boolValue) {
            *fileCount += 1;
        }
    }
    if (enumerationFailed) {
        return StarpointCnCdnDirectoryInvalid;
    }
    return *fileCount == 0 ? StarpointCnCdnDirectoryEmpty : StarpointCnCdnDirectoryReady;
}
// //// /检查 CN CDN 目录内容并拒绝符号链接 ////

// //// 读取已完成的 CN CDN 导入标记 [@x380kkm 2026-08-18] ////
static BOOL hasCnCdnImportMarker(NSURL *directoryURL) {
    NSURL *markerURL = [directoryURL URLByAppendingPathComponent:StarpointCnCdnImportMarkerName];
    NSDictionary *marker = [NSDictionary dictionaryWithContentsOfURL:markerURL];
    return [marker[@"schema"] isEqualToString:@"starpoint-cn-bundle-import"] &&
        [marker[@"version"] integerValue] == 1 &&
        [marker[@"file_count"] unsignedIntegerValue] > 0;
}
// //// /读取已完成的 CN CDN 导入标记 ////

// //// 原子导入 App bundle 内的 CN CDN [@x380kkm 2026-08-18] ////
static NSString *prepareCnCdnRoot(
    NSString *serviceRoot,
    StarpointPersonalServiceBootstrapStartResult *failureResult
) {
    BOOL isConfigured = NO;
    NSString *relativeBundlePath = configuredCnCdnBundlePath(&isConfigured);
    if (!isConfigured) {
        return nil;
    }
    if (relativeBundlePath == nil) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_CONFIGURATION_FAILED;
        return nil;
    }
    NSURL *sourceURL = resolveCnCdnBundleURL(relativeBundlePath);
    if (sourceURL == nil) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_CONFIGURATION_FAILED;
        return nil;
    }

    NSString *bundleMode = configuredCnCdnBundleMode();
    if (bundleMode != nil && ![bundleMode isEqualToString:StarpointCnCdnBundleModeDirect] &&
        ![bundleMode isEqualToString:@"import"]) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_CONFIGURATION_FAILED;
        return nil;
    }
    if ([bundleMode isEqualToString:StarpointCnCdnBundleModeDirect]) {
        NSUInteger directFileCount = 0;
        StarpointCnCdnDirectoryState directState = inspectCnCdnDirectory(
            sourceURL,
            &directFileCount
        );
        if (directState == StarpointCnCdnDirectoryMissing) {
            *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_MISSING;
            return nil;
        }
        if (directState == StarpointCnCdnDirectoryEmpty) {
            *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_EMPTY;
            return nil;
        }
        if (directState != StarpointCnCdnDirectoryReady) {
            *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED;
            return nil;
        }
        return sourceURL.path;
    }

    NSFileManager *fileManager = NSFileManager.defaultManager;
    NSURL *serviceRootURL = [NSURL fileURLWithPath:serviceRoot isDirectory:YES];
    NSURL *cdnParentURL = [serviceRootURL URLByAppendingPathComponent:@"cdn" isDirectory:YES];
    NSURL *targetURL = [cdnParentURL URLByAppendingPathComponent:@"cn" isDirectory:YES];
    NSError *error = nil;
    if (![fileManager createDirectoryAtURL:cdnParentURL
               withIntermediateDirectories:YES
                                attributes:nil
                                     error:&error]) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED;
        return nil;
    }
    if (hasCnCdnImportMarker(targetURL)) {
        NSUInteger existingFileCount = 0;
        if (inspectCnCdnDirectory(targetURL, &existingFileCount) == StarpointCnCdnDirectoryReady) {
            return targetURL.path;
        }
    }

    BOOL sourceIsDirectory = NO;
    if (![fileManager fileExistsAtPath:sourceURL.path isDirectory:&sourceIsDirectory]) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_MISSING;
        return nil;
    }
    if (!sourceIsDirectory) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_CONFIGURATION_FAILED;
        return nil;
    }

    NSString *stagingName = [NSString stringWithFormat:@"%@%@",
                                                       StarpointCnCdnStagingPrefix,
                                                       NSUUID.UUID.UUIDString];
    NSURL *stagingURL = [cdnParentURL URLByAppendingPathComponent:stagingName isDirectory:YES];
    [fileManager removeItemAtURL:stagingURL error:nil];
    BOOL copied = [fileManager copyItemAtURL:sourceURL toURL:stagingURL error:&error];
    if (!copied) {
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED;
        return nil;
    }

    NSUInteger fileCount = 0;
    StarpointCnCdnDirectoryState stagingState = inspectCnCdnDirectory(stagingURL, &fileCount);
    if (stagingState == StarpointCnCdnDirectoryEmpty) {
        [fileManager removeItemAtURL:stagingURL error:nil];
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_EMPTY;
        return nil;
    }
    if (stagingState != StarpointCnCdnDirectoryReady) {
        [fileManager removeItemAtURL:stagingURL error:nil];
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED;
        return nil;
    }

    NSDictionary *marker = @{
        @"schema": @"starpoint-cn-bundle-import",
        @"version": @1,
        @"file_count": @(fileCount),
    };
    NSURL *markerURL = [stagingURL URLByAppendingPathComponent:StarpointCnCdnImportMarkerName];
    if (![marker writeToURL:markerURL atomically:YES]) {
        [fileManager removeItemAtURL:stagingURL error:nil];
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED;
        return nil;
    }

    BOOL installed = NO;
    if ([fileManager fileExistsAtPath:targetURL.path]) {
        NSURL *resultingURL = nil;
        installed = [fileManager replaceItemAtURL:targetURL
                                    withItemAtURL:stagingURL
                                   backupItemName:nil
                                          options:0
                                 resultingItemURL:&resultingURL
                                            error:&error];
    } else {
        installed = [fileManager moveItemAtURL:stagingURL toURL:targetURL error:&error];
    }
    if (!installed) {
        [fileManager removeItemAtURL:stagingURL error:nil];
        *failureResult = STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED;
        return nil;
    }
    atomic_fetch_add_explicit(&cnCdnImportCount, 1, memory_order_acq_rel);
    return targetURL.path;
}
// //// /原子导入 App bundle 内的 CN CDN ////

// //// 从串行队列移除失效的个人服务 [@x380kkm 2026-08-22] ////
static void discardPersonalServiceOnQueue(void) {
    if (personalService == NULL) {
        return;
    }
    StarpointPersonalService *discardedPersonalService = personalService;
    personalService = NULL;
    starpoint_personal_service_stop(discardedPersonalService);
}
// //// /从串行队列移除失效的个人服务 ////

// //// 探测 loopback 个人服务的 HTTP 健康状态 [@x380kkm 2026-08-24] ////
static BOOL waitForPersonalServiceSocketEvent(int socketDescriptor, short events) {
    struct pollfd descriptor = {
        .fd = socketDescriptor,
        .events = events,
        .revents = 0,
    };
    int result = poll(
        &descriptor,
        1,
        StarpointPersonalServiceHealthProbeTimeoutMilliseconds
    );
    return result > 0 &&
        (descriptor.revents & (events | POLLERR | POLLHUP)) != 0;
}

static void closePersonalServiceHealthSocket(int socketDescriptor) {
    shutdown(socketDescriptor, SHUT_RDWR);
    close(socketDescriptor);
}

static StarpointPersonalServiceHealthState probePersonalServiceHealthOnQueue(void) {
    int socketDescriptor = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (socketDescriptor < 0) {
        return StarpointPersonalServiceHealthUnreachable;
    }

    int socketFlags = fcntl(socketDescriptor, F_GETFL, 0);
    if (socketFlags < 0 ||
        fcntl(socketDescriptor, F_SETFL, socketFlags | O_NONBLOCK) < 0) {
        closePersonalServiceHealthSocket(socketDescriptor);
        return StarpointPersonalServiceHealthUnreachable;
    }
#ifdef SO_NOSIGPIPE
    int suppressBrokenPipeSignal = 1;
    if (setsockopt(
            socketDescriptor,
            SOL_SOCKET,
            SO_NOSIGPIPE,
            &suppressBrokenPipeSignal,
            sizeof(suppressBrokenPipeSignal)
        ) != 0) {
        closePersonalServiceHealthSocket(socketDescriptor);
        return StarpointPersonalServiceHealthUnreachable;
    }
#endif

    struct sockaddr_in address = {
        .sin_len = sizeof(address),
        .sin_family = AF_INET,
        .sin_port = htons(StarpointPersonalServicePort),
        .sin_addr = {
            .s_addr = htonl(INADDR_LOOPBACK),
        },
    };
    int connectionResult = connect(
        socketDescriptor,
        (const struct sockaddr *)&address,
        sizeof(address)
    );
    if (connectionResult != 0) {
        if (errno != EINPROGRESS ||
            !waitForPersonalServiceSocketEvent(socketDescriptor, POLLOUT)) {
            closePersonalServiceHealthSocket(socketDescriptor);
            return StarpointPersonalServiceHealthUnreachable;
        }
        int socketError = 0;
        socklen_t socketErrorLength = sizeof(socketError);
        if (getsockopt(
                socketDescriptor,
                SOL_SOCKET,
                SO_ERROR,
                &socketError,
                &socketErrorLength
            ) != 0 || socketError != 0) {
            closePersonalServiceHealthSocket(socketDescriptor);
            return StarpointPersonalServiceHealthUnreachable;
        }
    }

    static const char request[] =
        "GET /health HTTP/1.1\r\n"
        "Host: 127.0.0.1\r\n"
        "Connection: close\r\n\r\n";
    if (!waitForPersonalServiceSocketEvent(socketDescriptor, POLLOUT) ||
        send(socketDescriptor, request, sizeof(request) - 1, 0) != sizeof(request) - 1 ||
        !waitForPersonalServiceSocketEvent(socketDescriptor, POLLIN)) {
        closePersonalServiceHealthSocket(socketDescriptor);
        return StarpointPersonalServiceHealthListening;
    }

    static const char healthyStatus[] = "HTTP/1.1 200";
    char response[64] = {0};
    ssize_t responseLength = recv(socketDescriptor, response, sizeof(response), 0);
    closePersonalServiceHealthSocket(socketDescriptor);
    BOOL isReady = responseLength >= (ssize_t)(sizeof(healthyStatus) - 1) &&
        memcmp(response, healthyStatus, sizeof(healthyStatus) - 1) == 0;
    return isReady
        ? StarpointPersonalServiceHealthReady
        : StarpointPersonalServiceHealthListening;
}
// //// /探测 loopback 个人服务的 HTTP 健康状态 ////

// //// 在串行队列中确保个人服务可用 [@x380kkm 2026-08-21] ////
static BOOL ensurePersonalServiceReadyOnQueue(void) {
    if (personalService != NULL) {
        if (starpoint_personal_service_is_running(personalService) != 0) {
            return YES;
        }
        discardPersonalServiceOnQueue();
    }

    NSString *root = createPersonalServiceRoot();
    if (root == nil) {
        atomic_store_explicit(
            &lastStartResult,
            STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SERVICE_ROOT_FAILED,
            memory_order_release
        );
        return NO;
    }
    StarpointPersonalServiceBootstrapStartResult preparationFailure =
        STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SERVICE_FAILED;
    NSString *cnAssetRoot = prepareCnCdnRoot(root, &preparationFailure);
    BOOL isCdnConfigured = NSProcessInfo.processInfo.environment[
        StarpointCnCdnBundleEnvironmentKey
    ] != nil || NSBundle.mainBundle.infoDictionary[StarpointCnCdnBundleInfoKey] != nil;
    if (isCdnConfigured && cnAssetRoot == nil) {
        atomic_store_explicit(&lastStartResult, preparationFailure, memory_order_release);
        NSLog(@"Starpoint personal service startup failed with code %d.",
              (int)preparationFailure);
        return NO;
    }
    if (cnAssetRoot != nil) {
        personalService = starpoint_personal_service_start_with_cdn_root(
            root.fileSystemRepresentation,
            cnAssetRoot.fileSystemRepresentation,
            StarpointPersonalServicePort
        );
    } else {
        personalService = starpoint_personal_service_start(
            root.fileSystemRepresentation,
            StarpointPersonalServicePort
        );
    }
    StarpointPersonalServiceBootstrapStartResult result = personalService != NULL
        ? STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SUCCEEDED
        : STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SERVICE_FAILED;
    atomic_store_explicit(&lastStartResult, result, memory_order_release);
    if (result != STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SUCCEEDED) {
        NSLog(@"Starpoint personal service startup failed with code %d.", (int)result);
    }
    return result == STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SUCCEEDED;
}
// //// /在串行队列中确保个人服务可用 ////

// //// 恢复前台个人服务 [@x380kkm 2026-08-24] ////
static BOOL recoverPersonalServiceForForegroundOnQueue(NSUInteger attempt) {
    BOOL isReportedRunning = personalService != NULL &&
        starpoint_personal_service_is_running(personalService) != 0;
    StarpointPersonalServiceHealthState healthState = isReportedRunning
        ? probePersonalServiceHealthOnQueue()
        : StarpointPersonalServiceHealthUnreachable;
    if (healthState == StarpointPersonalServiceHealthReady) {
        atomic_store_explicit(
            &lastStartResult,
            STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SUCCEEDED,
            memory_order_release
        );
        return YES;
    }

    BOOL shouldRestart = !isReportedRunning ||
        healthState == StarpointPersonalServiceHealthUnreachable ||
        (attempt > 0 && attempt % StarpointForegroundRecoveryRestartInterval == 0);
    if (!shouldRestart) {
        return NO;
    }
    discardPersonalServiceOnQueue();
    return ensurePersonalServiceReadyOnQueue() &&
        probePersonalServiceHealthOnQueue() == StarpointPersonalServiceHealthReady;
}
// //// /恢复前台个人服务 ////

// //// 在恢复窗口内排队前台个人服务恢复 [@x380kkm 2026-08-24] ////
static BOOL isPersonalServiceForegroundRecoveryCurrent(uint64_t generation) {
    return atomic_load_explicit(&foregroundRecoveryScheduled, memory_order_acquire) ==
        generation;
}

static void completePersonalServiceForegroundRecoveryOnQueue(
    uint64_t generation,
    BOOL isReady
) {
    if (!isPersonalServiceForegroundRecoveryCurrent(generation)) {
        return;
    }
    atomic_store_explicit(&foregroundRecoveryScheduled, 0, memory_order_release);

    NSArray *completions = [foregroundRecoveryCompletions copy];
    [foregroundRecoveryCompletions removeAllObjects];
    for (StarpointPersonalServiceRecoveryCompletion completion in completions) {
        completion(isReady);
    }
}

static void attemptPersonalServiceForegroundRecoveryOnQueue(
    NSUInteger attempt,
    uint64_t generation
) {
    if (!isPersonalServiceForegroundRecoveryCurrent(generation)) {
        return;
    }
    if (recoverPersonalServiceForForegroundOnQueue(attempt)) {
        completePersonalServiceForegroundRecoveryOnQueue(generation, YES);
        return;
    }

    NSUInteger nextAttempt = attempt + 1;
    if (nextAttempt >= StarpointForegroundRecoveryMaximumAttempts) {
        atomic_store_explicit(
            &lastStartResult,
            STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_SERVICE_FAILED,
            memory_order_release
        );
        completePersonalServiceForegroundRecoveryOnQueue(generation, NO);
        NSLog(@"Starpoint personal service foreground recovery exhausted.");
        return;
    }

    dispatch_after(
        dispatch_time(
            DISPATCH_TIME_NOW,
            (int64_t)StarpointForegroundRecoveryRetryDelayNanoseconds
        ),
        personalServiceQueue,
        ^{
          attemptPersonalServiceForegroundRecoveryOnQueue(nextAttempt, generation);
        }
    );
}

static void schedulePersonalServiceForegroundRecovery(
    StarpointPersonalServiceRecoveryCompletion completion
) {
    if (personalServiceQueue == nil) {
        if (completion != nil) {
            completion(NO);
        }
        return;
    }
    dispatch_async(personalServiceQueue, ^{
      if (foregroundRecoveryCompletions == nil) {
          foregroundRecoveryCompletions = [[NSMutableArray alloc] init];
      }
      if (completion != nil) {
          [foregroundRecoveryCompletions addObject:[completion copy]];
      }
      if (atomic_load_explicit(&foregroundRecoveryScheduled, memory_order_acquire) != 0) {
          return;
      }

      uint64_t generation = atomic_fetch_add_explicit(
          &foregroundRecoveryGeneration,
          1,
          memory_order_acq_rel
      ) + 1;
      atomic_store_explicit(
          &foregroundRecoveryScheduled,
          generation,
          memory_order_release
      );
      attemptPersonalServiceForegroundRecoveryOnQueue(0, generation);
    });
}

static void cancelPersonalServiceForegroundRecovery(void) {
    if (personalServiceQueue == nil) {
        return;
    }
    dispatch_async(personalServiceQueue, ^{
      uint64_t generation = atomic_load_explicit(
          &foregroundRecoveryScheduled,
          memory_order_acquire
      );
      if (generation != 0) {
          completePersonalServiceForegroundRecoveryOnQueue(generation, NO);
      }
    });
}
// //// /在恢复窗口内排队前台个人服务恢复 ////

// //// 异步确保个人服务可用 [@x380kkm 2026-08-21] ////
static void startPersonalService(void) {
    dispatch_async(personalServiceQueue, ^{
      ensurePersonalServiceReadyOnQueue();
    });
}
// //// /异步确保个人服务可用 ////

// //// 等待个人服务进入可用状态 [@x380kkm 2026-08-21] ////
static BOOL ensurePersonalServiceReady(void) {
    __block BOOL isReady = NO;
    if (personalServiceQueue == nil) {
        return isReady;
    }
    dispatch_sync(personalServiceQueue, ^{
      isReady = ensurePersonalServiceReadyOnQueue();
    });
    return isReady;
}
// //// /等待个人服务进入可用状态 ////

// //// 启动 iOS 宿主使用的个人服务 [@x380kkm 2026-07-24] ////
void starpoint_personal_service_bootstrap_start(void) {
    if (personalServiceQueue != nil) {
        startPersonalService();
    }
}
// //// /启动 iOS 宿主使用的个人服务 ////

// //// 结束指定状态切换的后台执行 [@x380kkm 2026-08-21] ////
static void endPersonalServiceBackgroundExecution(
    UIApplication *application,
    uint64_t generation
) {
    if (!NSThread.isMainThread) {
        dispatch_async(dispatch_get_main_queue(), ^{
          endPersonalServiceBackgroundExecution(application, generation);
        });
        return;
    }
    if (generation != backgroundExecutionGeneration ||
        backgroundExecutionTask == UIBackgroundTaskInvalid) {
        return;
    }
    UIBackgroundTaskIdentifier task = backgroundExecutionTask;
    backgroundExecutionTask = UIBackgroundTaskInvalid;
    [application endBackgroundTask:task];
}
// //// /结束指定状态切换的后台执行 ////

// //// 排队前台激活收尾 [@x380kkm 2026-08-24] ////
static void schedulePersonalServiceForegroundActivationCompletion(
    UIApplication *application,
    uint64_t backgroundGeneration
) {
    schedulePersonalServiceForegroundRecovery(^(__unused BOOL isReady) {
      dispatch_async(dispatch_get_main_queue(), ^{
        endPersonalServiceBackgroundExecution(application, backgroundGeneration);
        reconcileManagementEntry(application);
      });
    });
}
// //// /排队前台激活收尾 ////

// //// 为状态切换保留有界后台执行 [@x380kkm 2026-08-21] ////
static uint64_t beginPersonalServiceBackgroundExecution(UIApplication *application) {
    if (backgroundExecutionTask != UIBackgroundTaskInvalid) {
        return backgroundExecutionGeneration;
    }
    backgroundExecutionGeneration += 1;
    uint64_t generation = backgroundExecutionGeneration;
    backgroundExecutionTask = [application
        beginBackgroundTaskWithName:@"StarpointPersonalServiceTransition"
                  expirationHandler:^{
                    endPersonalServiceBackgroundExecution(application, generation);
                  }];
    return generation;
}
// //// /为状态切换保留有界后台执行 ////

// //// 在后台时限内提交 SQLite WAL [@x380kkm 2026-07-22] ////
static void flushPersonalServiceForBackground(
    UIApplication *application,
    uint64_t generation
) {
    dispatch_async(personalServiceQueue, ^{
      int32_t flushResult = -1;
      if (personalService != NULL) {
          flushResult = starpoint_personal_service_flush(personalService);
      }
      atomic_store_explicit(&lastBackgroundFlushResult, flushResult, memory_order_release);
      atomic_fetch_add_explicit(&backgroundFlushCount, 1, memory_order_acq_rel);
      dispatch_async(dispatch_get_main_queue(), ^{
        endPersonalServiceBackgroundExecution(application, generation);
      });
    });
}
// //// /在后台时限内提交 SQLite WAL ////

// //// 在正常终止前关闭个人服务 [@x380kkm 2026-07-22] ////
static void stopPersonalService(void) {
    if (personalServiceQueue == nil) {
        return;
    }
    dispatch_sync(personalServiceQueue, ^{
      if (personalService == NULL) {
          return;
      }
      starpoint_personal_service_flush(personalService);
      starpoint_personal_service_stop(personalService);
      personalService = NULL;
    });
}
// //// /在正常终止前关闭个人服务 ////

// //// 读取当前 App 用于呈现页面的窗口 [@x380kkm 2026-07-24] ////
static UIWindow *applicationPresentationWindow(UIApplication *application) {
    if (@available(iOS 13.0, *)) {
        for (UIScene *scene in application.connectedScenes) {
            if (![scene isKindOfClass:[UIWindowScene class]]) {
                continue;
            }
            if (scene.activationState != UISceneActivationStateForegroundActive &&
                scene.activationState != UISceneActivationStateForegroundInactive) {
                continue;
            }
            UIWindowScene *windowScene = (UIWindowScene *)scene;
            for (UIWindow *window in windowScene.windows) {
                if (window.isKeyWindow && !window.hidden) {
                    return window;
                }
            }
            for (UIWindow *window in windowScene.windows) {
                if (!window.hidden && window.windowLevel == UIWindowLevelNormal) {
                    return window;
                }
            }
        }
    }

    id<UIApplicationDelegate> delegate = application.delegate;
    if ([delegate respondsToSelector:@selector(window)]) {
        UIWindow *delegateWindow = delegate.window;
        if (delegateWindow != nil && !delegateWindow.hidden) {
            return delegateWindow;
        }
    }

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    UIWindow *keyWindow = application.keyWindow;
    if (keyWindow != nil && !keyWindow.hidden) {
        return keyWindow;
    }
    for (UIWindow *window in application.windows) {
        if (!window.hidden && window.windowLevel == UIWindowLevelNormal) {
            return window;
        }
    }
#pragma clang diagnostic pop
    return nil;
}
// //// /读取当前 App 用于呈现页面的窗口 ////

// //// 递归取得最上层的可呈现控制器 [@x380kkm 2026-07-24] ////
static UIViewController *topViewController(UIViewController *controller) {
    if (controller == nil) {
        return nil;
    }
    UIViewController *presented = controller.presentedViewController;
    if (presented != nil && !presented.isBeingDismissed) {
        return topViewController(presented);
    }
    if ([controller isKindOfClass:[UINavigationController class]]) {
        UIViewController *visible = ((UINavigationController *)controller).visibleViewController;
        if (visible != nil) {
            return topViewController(visible);
        }
    }
    if ([controller isKindOfClass:[UITabBarController class]]) {
        UIViewController *selected = ((UITabBarController *)controller).selectedViewController;
        if (selected != nil) {
            return topViewController(selected);
        }
    }
    if ([controller isKindOfClass:[UISplitViewController class]]) {
        UIViewController *visible = ((UISplitViewController *)controller).viewControllers.lastObject;
        if (visible != nil) {
            return topViewController(visible);
        }
    }
    for (UIViewController *child in controller.childViewControllers.reverseObjectEnumerator) {
        if (child.isViewLoaded && child.view.window != nil) {
            return topViewController(child);
        }
    }
    return controller;
}
// //// /递归取得最上层的可呈现控制器 ////

// //// 创建本地管理页面地址 [@x380kkm 2026-08-20] ////
static NSURL *managementPageURL(void) {
    NSURLComponents *components = [[NSURLComponents alloc] init];
    components.scheme = @"http";
    components.host = @"127.0.0.1";
    components.port = @(starpoint_personal_service_bootstrap_port());
    components.path = @"/manage/";
    return components.URL;
}
// //// /创建本地管理页面地址 ////

// //// 在最上层控制器安全呈现 Safari 管理页面 [@x380kkm 2026-07-24] ////
static void presentManagementPage(void) {
    if (presentedManagementController != nil) {
        return;
    }
    NSURL *managementURL = managementPageURL();
    if (managementURL == nil) {
        return;
    }
    UIApplication *application = UIApplication.sharedApplication;
    UIWindow *window = applicationPresentationWindow(application);
    UIViewController *presenter = topViewController(window.rootViewController);
    if (presenter == nil || presenter.isBeingDismissed || presenter.isBeingPresented ||
        presenter.presentedViewController != nil || presenter.viewIfLoaded.window == nil) {
        return;
    }
    SFSafariViewController *controller = [[SFSafariViewController alloc] initWithURL:managementURL];
    presentedManagementController = controller;
    [presenter presentViewController:controller animated:YES completion:nil];
}
// //// /在最上层控制器安全呈现 Safari 管理页面 ////

// //// 移除标题页管理按钮 [@x380kkm 2026-08-20] ////
static void removeManagementEntryButton(void) {
    [managementEntryButton removeFromSuperview];
    managementEntryButton = nil;
    managementEntryTarget = nil;
}
// //// /移除标题页管理按钮 ////

// //// 按标题场景状态对账管理按钮 [@x380kkm 2026-08-20] ////
static void reconcileManagementEntry(UIApplication *application) {
    if (atomic_load_explicit(&managementEntryVisible, memory_order_acquire) == 0) {
        removeManagementEntryButton();
        return;
    }

    UIWindow *window = applicationPresentationWindow(application);
    if (window == nil) {
        return;
    }

    if (managementEntryButton.superview == window) {
        [window bringSubviewToFront:managementEntryButton];
        return;
    }

    removeManagementEntryButton();
    managementEntryTarget = [[StarpointManagementEntryTarget alloc] init];
    managementEntryButton = [UIButton buttonWithType:UIButtonTypeSystem];
    managementEntryButton.translatesAutoresizingMaskIntoConstraints = NO;
    managementEntryButton.accessibilityIdentifier = @"starpoint.management.entry";
    managementEntryButton.accessibilityLabel = @"管理";
    managementEntryButton.backgroundColor = [UIColor colorWithWhite:0.08 alpha:0.72];
    managementEntryButton.layer.cornerRadius = 22.0;
    managementEntryButton.titleLabel.font = [UIFont systemFontOfSize:11.0 weight:UIFontWeightSemibold];
    [managementEntryButton setTitle:@"管理" forState:UIControlStateNormal];
    [managementEntryButton setTitleColor:UIColor.whiteColor forState:UIControlStateNormal];
    [managementEntryButton addTarget:managementEntryTarget
                              action:@selector(openManagementPage)
                    forControlEvents:UIControlEventTouchUpInside];
    [window addSubview:managementEntryButton];

    UILayoutGuide *safeArea = window.safeAreaLayoutGuide;
    [NSLayoutConstraint activateConstraints:@[
      [managementEntryButton.widthAnchor constraintEqualToConstant:44.0],
      [managementEntryButton.heightAnchor constraintEqualToConstant:44.0],
      [managementEntryButton.trailingAnchor constraintEqualToAnchor:safeArea.trailingAnchor constant:-12.0],
      [managementEntryButton.topAnchor constraintEqualToAnchor:safeArea.topAnchor constant:12.0],
    ]];
}
// //// /按标题场景状态对账管理按钮 ////

// //// 接收标题场景的管理入口状态 [@x380kkm 2026-08-20] ////
void starpoint_personal_service_bootstrap_set_management_entry_visible(int32_t visible) {
    atomic_store_explicit(&managementEntryVisible, visible == 0 ? 0 : 1, memory_order_release);
    void (^reconcile)(void) = ^{
      reconcileManagementEntry(UIApplication.sharedApplication);
    };
    if (NSThread.isMainThread) {
        reconcile();
    } else {
        dispatch_async(dispatch_get_main_queue(), reconcile);
    }
}

int32_t starpoint_personal_service_bootstrap_management_entry_visible(void) {
    return atomic_load_explicit(&managementEntryVisible, memory_order_acquire);
}
// //// /接收标题场景的管理入口状态 ////

// //// 提交 iOS 个人服务的持久化状态 [@x380kkm 2026-07-24] ////
int32_t starpoint_personal_service_bootstrap_flush(void) {
    __block int32_t result = 0;
    if (personalServiceQueue == nil) {
        return result;
    }
    dispatch_sync(personalServiceQueue, ^{
      if (personalService != NULL) {
          result = starpoint_personal_service_flush(personalService);
      }
    });
    return result;
}
// //// /提交 iOS 个人服务的持久化状态 ////

// //// 关闭 iOS 宿主使用的个人服务 [@x380kkm 2026-07-24] ////
void starpoint_personal_service_bootstrap_stop(void) {
    stopPersonalService();
}
// //// /关闭 iOS 宿主使用的个人服务 ////

// //// 绑定 App 生命周期通知 [@x380kkm 2026-08-24] ////
__attribute__((constructor)) static void installPersonalServiceBootstrap(void) {
    installNativeLoginURLRouting();
    personalServiceQueue = dispatch_queue_create("dev.starpoint.personal-service", DISPATCH_QUEUE_SERIAL);
    dispatch_async(dispatch_get_main_queue(), ^{
      NSNotificationCenter *notifications = NSNotificationCenter.defaultCenter;
      [notifications addObserverForName:UIApplicationWillResignActiveNotification
                                 object:nil
                                  queue:NSOperationQueue.mainQueue
                             usingBlock:^(__unused NSNotification *notification) {
                               cancelPersonalServiceForegroundRecovery();
                               beginPersonalServiceBackgroundExecution(
                                   UIApplication.sharedApplication
                               );
                             }];
      [notifications addObserverForName:UIApplicationDidEnterBackgroundNotification
                                 object:nil
                                  queue:NSOperationQueue.mainQueue
                             usingBlock:^(__unused NSNotification *notification) {
                               UIApplication *application = UIApplication.sharedApplication;
                               uint64_t generation =
                                   beginPersonalServiceBackgroundExecution(application);
                               flushPersonalServiceForBackground(application, generation);
                             }];
      [notifications addObserverForName:UIApplicationWillEnterForegroundNotification
                                 object:nil
                                  queue:NSOperationQueue.mainQueue
                             usingBlock:^(__unused NSNotification *notification) {
                               atomic_fetch_add_explicit(
                                   &foregroundResumeCount,
                                   1,
                                   memory_order_acq_rel
                               );
                               schedulePersonalServiceForegroundRecovery(nil);
                             }];
      [notifications addObserverForName:UIApplicationDidBecomeActiveNotification
                                 object:nil
                                  queue:NSOperationQueue.mainQueue
                             usingBlock:^(__unused NSNotification *notification) {
                               UIApplication *application = UIApplication.sharedApplication;
                               uint64_t backgroundGeneration = backgroundExecutionGeneration;
                               schedulePersonalServiceForegroundActivationCompletion(
                                   application,
                                   backgroundGeneration
                               );
                             }];
      [notifications addObserverForName:UIApplicationWillTerminateNotification
                                 object:nil
                                  queue:NSOperationQueue.mainQueue
                             usingBlock:^(__unused NSNotification *notification) {
                               stopPersonalService();
                             }];
      ensurePersonalServiceReady();
      reconcileManagementEntry(UIApplication.sharedApplication);
    });
}
// //// /绑定 App 生命周期通知 ////
