// audience: internal
// # personal-service-diagnostic
//
// 该 App 加载个人服务 Framework 和同源 Web 管理页. Simulator 由外部测试检查,
// iPhone 构建在独立沙盒中自检管理鉴权和持久化.

#import <UIKit/UIKit.h>
#import <WebKit/WebKit.h>

#import <PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.h>

static NSString *const StarpointDiagnosticGenerationKey = @"StarpointDiagnosticGeneration";
static NSString *const StarpointDiagnosticRunIDKey = @"run_id";
static NSString *const StarpointDiagnosticStateKey = @"state";
static NSString *const StarpointDiagnosticStageKey = @"stage";
static NSString *const StarpointDiagnosticErrorCodeKey = @"error_code";
static NSString *const StarpointDiagnosticGenerationBeforeKey = @"generation_before";
static NSString *const StarpointDiagnosticGenerationAfterKey = @"generation_after";
static NSString *const StarpointDiagnosticLifecycleStateKey = @"lifecycle_state";
static NSString *const StarpointDiagnosticBackgroundFlushCountKey = @"background_flush_count";
static NSString *const StarpointDiagnosticBackgroundFlushResultKey = @"background_flush_result";
static NSString *const StarpointDiagnosticForegroundResumeCountKey = @"foreground_resume_count";
static NSString *const StarpointManagementPageStateKey = @"management_page_state";
static NSString *const StarpointManagementEntryAccessibilityIdentifier = @"starpoint.management.entry";
static NSString *const StarpointOpenManagementEnvironmentKey = @"STARPOINT_OPEN_MANAGEMENT";

typedef void (^StarpointDiagnosticCompletion)(NSDictionary *result, NSError *error);
typedef void (^StarpointDiagnosticJSONCompletion)(id result, NSError *error);
typedef void (^StarpointDiagnosticStatusCompletion)(NSError *error);

@interface StarpointDiagnosticAppDelegate : UIResponder <UIApplicationDelegate, WKNavigationDelegate>
@property(nonatomic, strong) UIWindow *window;
@property(nonatomic, strong) UILabel *statusLabel;
@property(nonatomic, strong) UIButton *managementButton;
@property(nonatomic, copy) NSString *diagnosticRunID;
@property(nonatomic) uint64_t backgroundFlushCountBefore;
@property(nonatomic) uint64_t foregroundResumeCountBefore;
@property(nonatomic) BOOL lifecyclePending;
@end

// //// 运行 iOS 个人服务诊断 [@x380kkm 2026-07-23] ////
@implementation StarpointDiagnosticAppDelegate

// //// 读取个人服务的 loopback 地址 [@x380kkm 2026-07-24] ////
- (NSString *)managementBaseURL {
    return [NSString stringWithFormat:@"http://127.0.0.1:%u",
                                      (unsigned)starpoint_personal_service_bootstrap_port()];
}
// //// /读取个人服务的 loopback 地址 ////

- (void)reportStatus:(NSString *)status color:(UIColor *)color {
    dispatch_async(dispatch_get_main_queue(), ^{
      self.statusLabel.text = status;
      self.statusLabel.textColor = color;
    });
}

- (void)writeDiagnosticState:(NSString *)state
                       stage:(NSString *)stage
                   errorCode:(NSString *)errorCode
            generationBefore:(NSNumber *)generationBefore
             generationAfter:(NSNumber *)generationAfter {
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    [defaults setObject:self.diagnosticRunID forKey:StarpointDiagnosticRunIDKey];
    [defaults setObject:state forKey:StarpointDiagnosticStateKey];
    [defaults setObject:stage forKey:StarpointDiagnosticStageKey];
    if (errorCode == nil) {
        [defaults removeObjectForKey:StarpointDiagnosticErrorCodeKey];
    } else {
        [defaults setObject:errorCode forKey:StarpointDiagnosticErrorCodeKey];
    }
    if (generationBefore == nil) {
        [defaults removeObjectForKey:StarpointDiagnosticGenerationBeforeKey];
    } else {
        [defaults setObject:generationBefore forKey:StarpointDiagnosticGenerationBeforeKey];
    }
    if (generationAfter == nil) {
        [defaults removeObjectForKey:StarpointDiagnosticGenerationAfterKey];
    } else {
        [defaults setObject:generationAfter forKey:StarpointDiagnosticGenerationAfterKey];
    }
    [defaults synchronize];
}

- (void)failDiagnosticAtStage:(NSString *)stage
                    errorCode:(NSString *)errorCode
                       status:(NSString *)status
             generationBefore:(NSNumber *)generationBefore {
    [self writeDiagnosticState:@"failed"
                         stage:stage
                     errorCode:errorCode
              generationBefore:generationBefore
               generationAfter:nil];
    [self reportStatus:status color:UIColor.redColor];
}

- (void)requestJSONPath:(NSString *)path
                 method:(NSString *)method
            bearerToken:(NSString *)bearerToken
               jsonBody:(NSDictionary *)jsonBody
         expectedStatus:(NSInteger)expectedStatus
             completion:(StarpointDiagnosticJSONCompletion)completion {
    NSURL *url = [NSURL URLWithString:[[self managementBaseURL] stringByAppendingString:path]];
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url];
    request.HTTPMethod = method;
    request.timeoutInterval = 2.0;
    if (bearerToken != nil) {
        [request setValue:[NSString stringWithFormat:@"Bearer %@", bearerToken]
            forHTTPHeaderField:@"Authorization"];
    }
    if (jsonBody != nil) {
        NSError *bodyError = nil;
        request.HTTPBody = [NSJSONSerialization dataWithJSONObject:jsonBody options:0 error:&bodyError];
        if (bodyError != nil) {
            completion(nil, bodyError);
            return;
        }
        [request setValue:@"application/json" forHTTPHeaderField:@"Content-Type"];
    }
    NSURLSessionDataTask *task = [NSURLSession.sharedSession
        dataTaskWithRequest:request
          completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
            if (error != nil) {
                completion(nil, error);
                return;
            }
            NSHTTPURLResponse *httpResponse = (NSHTTPURLResponse *)response;
            BOOL isHTTPResponse = [httpResponse isKindOfClass:NSHTTPURLResponse.class];
            NSInteger statusCode = isHTTPResponse ? httpResponse.statusCode : -1;
            if (!isHTTPResponse || statusCode != expectedStatus) {
                NSError *statusError = [NSError
                    errorWithDomain:@"dev.starpoint.personal-service-diagnostic"
                               code:statusCode
                           userInfo:@{NSLocalizedDescriptionKey : @"个人服务返回非预期状态."}];
                completion(nil, statusError);
                return;
            }
            NSError *jsonError = nil;
            id result = [NSJSONSerialization JSONObjectWithData:data options:0 error:&jsonError];
            if (result == nil) {
                NSError *responseError = jsonError ?: [NSError
                    errorWithDomain:@"dev.starpoint.personal-service-diagnostic"
                               code:-1
                           userInfo:@{NSLocalizedDescriptionKey : @"个人服务返回无效 JSON."}];
                completion(nil, responseError);
                return;
            }
            completion(result, nil);
          }];
    [task resume];
}

- (void)requestPath:(NSString *)path
              method:(NSString *)method
         bearerToken:(NSString *)bearerToken
         completion:(StarpointDiagnosticCompletion)completion {
    [self requestJSONPath:path
                   method:method
              bearerToken:bearerToken
                 jsonBody:nil
           expectedStatus:200
               completion:^(id result, NSError *error) {
                 if (error != nil || ![result isKindOfClass:NSDictionary.class]) {
                     NSError *responseError = error ?: [NSError
                         errorWithDomain:@"dev.starpoint.personal-service-diagnostic"
                                    code:-1
                                userInfo:@{NSLocalizedDescriptionKey : @"个人服务返回无效 JSON 对象."}];
                     completion(nil, responseError);
                     return;
                 }
                 completion((NSDictionary *)result, nil);
               }];
}

// //// 通过 CN signup 为空数据容器创建默认本地存档 [@x380kkm 2026-08-19] ////
- (void)requestDiagnosticSignup:(StarpointDiagnosticStatusCompletion)completion {
    NSURL *url = [NSURL URLWithString:[[self managementBaseURL]
        stringByAppendingString:@"/api/index.php/tool/signup"]];
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url];
    request.HTTPMethod = @"POST";
    request.timeoutInterval = 2.0;
    [request setValue:@"application/x-www-form-urlencoded"
        forHTTPHeaderField:@"Content-Type"];
    // CN signup 的最小请求是命名 MessagePack 后再 Base64 包装.
    request.HTTPBody = [@"galkZXZpY2VfaWQB" dataUsingEncoding:NSASCIIStringEncoding];
    NSURLSessionDataTask *task = [NSURLSession.sharedSession
        dataTaskWithRequest:request
          completionHandler:^(__unused NSData *data, NSURLResponse *response, NSError *error) {
            if (error != nil) {
                completion(error);
                return;
            }
            NSHTTPURLResponse *httpResponse = (NSHTTPURLResponse *)response;
            if (![httpResponse isKindOfClass:NSHTTPURLResponse.class]
                || httpResponse.statusCode != 200) {
                completion([NSError
                    errorWithDomain:@"dev.starpoint.personal-service-diagnostic"
                               code:httpResponse.statusCode
                           userInfo:@{NSLocalizedDescriptionKey : @"CN signup 返回非 200 状态."}]);
                return;
            }
            completion(nil);
          }];
    [task resume];
}
// //// /通过 CN signup 为空数据容器创建默认本地存档 ////

- (NSString *)managementToken {
    size_t requiredLength = starpoint_personal_service_bootstrap_copy_management_token(NULL, 0);
    if (requiredLength < 2) {
        return nil;
    }
    NSMutableData *token = [NSMutableData dataWithLength:requiredLength];
    size_t copiedLength = starpoint_personal_service_bootstrap_copy_management_token(
        token.mutableBytes,
        token.length
    );
    if (copiedLength != requiredLength) {
        return nil;
    }
    return [NSString stringWithUTF8String:token.bytes];
}

- (void)setManagementAvailable:(BOOL)available {
    dispatch_async(dispatch_get_main_queue(), ^{
      self.managementButton.enabled = available;
    });
}

- (BOOL)opensManagementPageAutomatically {
    NSString *value = NSProcessInfo.processInfo.environment[StarpointOpenManagementEnvironmentKey];
    return [value isEqualToString:@"1"];
}

- (void)waitForManagementPageAttempt:(NSInteger)attempt {
    [self requestPath:@"/health"
               method:@"GET"
          bearerToken:nil
           completion:^(__unused NSDictionary *health, NSError *healthError) {
             if (healthError == nil) {
                 [self setManagementAvailable:YES];
                 if ([self opensManagementPageAutomatically]) {
                     dispatch_async(dispatch_get_main_queue(), ^{
                       [self openManagement];
                     });
                 }
                 return;
             }
             if (attempt >= 40) {
                 return;
             }
             dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 250 * NSEC_PER_MSEC),
                            dispatch_get_main_queue(), ^{
                              [self waitForManagementPageAttempt:attempt + 1];
                            });
           }];
}

- (void)closeManagement {
    [self.window.rootViewController dismissViewControllerAnimated:YES completion:nil];
}

- (void)openManagement {
    NSURLComponents *components = [NSURLComponents
        componentsWithString:[[self managementBaseURL] stringByAppendingString:@"/manage/"]];
    if ([self opensManagementPageAutomatically]) {
        components.fragment = @"mail-management";
    }

    WKWebViewConfiguration *configuration = [[WKWebViewConfiguration alloc] init];
    configuration.websiteDataStore = [WKWebsiteDataStore nonPersistentDataStore];
    WKWebView *webView = [[WKWebView alloc] initWithFrame:CGRectZero configuration:configuration];
    webView.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    webView.navigationDelegate = self;

    UIViewController *managementController = [[UIViewController alloc] init];
    managementController.title = @"个人服务管理";
    managementController.view = webView;
    managementController.navigationItem.rightBarButtonItem = [[UIBarButtonItem alloc]
        initWithBarButtonSystemItem:UIBarButtonSystemItemDone
                             target:self
                             action:@selector(closeManagement)];
    UINavigationController *navigation = [[UINavigationController alloc]
        initWithRootViewController:managementController];
    navigation.modalPresentationStyle = UIModalPresentationFullScreen;
    [self.window.rootViewController presentViewController:navigation animated:YES completion:nil];
    [NSUserDefaults.standardUserDefaults setObject:@"loading" forKey:StarpointManagementPageStateKey];
    [NSUserDefaults.standardUserDefaults synchronize];
    [webView loadRequest:[NSURLRequest requestWithURL:components.URL
                                         cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                     timeoutInterval:5.0]];
}

// //// 记录管理页面完成或失败状态 [@x380kkm 2026-08-20] ////
- (void)waitForManagementPageRendering:(WKWebView *)webView attempt:(NSInteger)attempt {
    NSString *readinessScript =
        @"(() => { const state = document.querySelector('#mail-catalog-state'); "
         @"const catalogLoaded = state?.dataset.loadState === 'loaded' && "
         @"Number(state?.dataset.itemCount || 0) > 0; "
         @"if (catalogLoaded) document.querySelector('#mail-management')?.scrollIntoView(); "
         @"const decodedImage = Array.from(document.querySelectorAll("
         @"'#mail-management img[data-reward-image-source=\"catalog\"]'))"
         @".some((image) => image.dataset.rewardImageState === 'loaded' && "
         @"image.complete && image.naturalWidth > 0 && image.currentSrc.includes('.png')); "
         @"return document.querySelector('#connection-state')?.textContent === '已连接本机' && "
         @"catalogLoaded && decodedImage; })()";
    [webView evaluateJavaScript:readinessScript completionHandler:^(id result, NSError *error) {
      if (error == nil && [result boolValue]) {
          dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 250 * NSEC_PER_MSEC),
                         dispatch_get_main_queue(), ^{
                           [NSUserDefaults.standardUserDefaults setObject:@"loaded"
                                                                   forKey:StarpointManagementPageStateKey];
                           [NSUserDefaults.standardUserDefaults synchronize];
                         });
          return;
      }
      if (attempt >= 40) {
          [NSUserDefaults.standardUserDefaults setObject:@"failed"
                                                  forKey:StarpointManagementPageStateKey];
          [NSUserDefaults.standardUserDefaults synchronize];
          return;
      }
      dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 250 * NSEC_PER_MSEC),
                     dispatch_get_main_queue(), ^{
                       [self waitForManagementPageRendering:webView attempt:attempt + 1];
                     });
    }];
}

- (void)webView:(WKWebView *)webView didFinishNavigation:(__unused WKNavigation *)navigation {
    [self waitForManagementPageRendering:webView attempt:0];
}

- (void)webView:(__unused WKWebView *)webView
    didFailNavigation:(__unused WKNavigation *)navigation
             withError:(__unused NSError *)error {
    [NSUserDefaults.standardUserDefaults setObject:@"failed" forKey:StarpointManagementPageStateKey];
    [NSUserDefaults.standardUserDefaults synchronize];
}

- (void)webView:(__unused WKWebView *)webView
    didFailProvisionalNavigation:(__unused WKNavigation *)navigation
                        withError:(__unused NSError *)error {
    [NSUserDefaults.standardUserDefaults setObject:@"failed" forKey:StarpointManagementPageStateKey];
    [NSUserDefaults.standardUserDefaults synchronize];
}
// //// /记录管理页面完成或失败状态 ////

- (void)runPersistenceDiagnosticFromGeneration:(NSInteger)generation {
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    NSNumber *previousGeneration = [defaults objectForKey:StarpointDiagnosticGenerationKey];
    if (previousGeneration != nil && generation < previousGeneration.integerValue) {
        [self failDiagnosticAtStage:@"state_increment"
                          errorCode:@"PERSISTENCE_REGRESSION"
                             status:@"失败: SQLite 代次在重新启动后回退."
                   generationBefore:@(generation)];
        return;
    }

    [self writeDiagnosticState:@"running"
                         stage:@"state_increment"
                     errorCode:nil
              generationBefore:@(generation)
               generationAfter:nil];

    [self requestPath:@"/v1/state/increment"
               method:@"POST"
          bearerToken:nil
           completion:^(NSDictionary *incremented, NSError *incrementError) {
             NSInteger nextGeneration = [incremented[@"generation"] integerValue];
             if (incrementError != nil || nextGeneration != generation + 1) {
                 [self failDiagnosticAtStage:@"state_increment"
                                   errorCode:@"PERSISTENCE_REGRESSION"
                                      status:@"失败: 无法更新 SQLite 代次."
                            generationBefore:@(generation)];
                 return;
             }
             [self writeDiagnosticState:@"running"
                                  stage:@"checkpoint"
                              errorCode:nil
                       generationBefore:@(generation)
                        generationAfter:@(nextGeneration)];
             [self requestPath:@"/v1/checkpoint"
                          method:@"POST"
                     bearerToken:nil
                      completion:^(__unused NSDictionary *checkpoint, NSError *checkpointError) {
                        if (checkpointError != nil) {
                            [self failDiagnosticAtStage:@"checkpoint"
                                              errorCode:@"CHECKPOINT_FAILED"
                                                 status:@"失败: 无法提交 SQLite checkpoint."
                                       generationBefore:@(generation)];
                            return;
                        }
                        [defaults setInteger:nextGeneration
                                      forKey:StarpointDiagnosticGenerationKey];
                        NSString *status = [NSString
                            stringWithFormat:@"通过: 管理鉴权和 loopback 正常, SQLite 代次 %ld -> %ld.\n"
                                             @"切到后台后重新打开, 可复核暂停时刷盘.",
                                             (long)generation,
                                             (long)nextGeneration];
                         [self writeDiagnosticState:@"passed"
                                             stage:@"complete"
                                         errorCode:nil
                                  generationBefore:@(generation)
                                   generationAfter:@(nextGeneration)];
                         [self reportStatus:status color:UIColor.greenColor];
                              }];
           }];
}

// //// 验证 iOS 内嵌服务的存档, 时间和邮件管理 [@x380kkm 2026-08-19] ////
- (void)runManagementFeatureDiagnosticFromGeneration:(NSInteger)generation
                                                token:(NSString *)managementToken {
    [self writeDiagnosticState:@"running"
                         stage:@"management_features"
                     errorCode:nil
              generationBefore:@(generation)
               generationAfter:nil];
    [self requestJSONPath:@"/v1/local-saves"
                   method:@"GET"
              bearerToken:managementToken
                 jsonBody:nil
           expectedStatus:200
               completion:^(id state, NSError *stateError) {
                 NSArray *slots = [state isKindOfClass:NSDictionary.class] ? state[@"slots"] : nil;
                 NSDictionary *slot = [slots isKindOfClass:NSArray.class] ? slots.firstObject : nil;
                 NSNumber *slotID = [slot isKindOfClass:NSDictionary.class] ? slot[@"id"] : nil;
                 if (stateError != nil) {
                     [self failDiagnosticAtStage:@"management_features"
                                       errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                          status:@"失败: iOS 内嵌服务没有可管理的本地存档槽."
                                generationBefore:@(generation)];
                     return;
                 }

                 if (![slotID isKindOfClass:NSNumber.class]) {
                     [self requestDiagnosticSignup:^(NSError *signupError) {
                       if (signupError != nil) {
                           [self failDiagnosticAtStage:@"management_features"
                                             errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                                status:@"失败: iOS 内嵌服务无法创建默认本地存档槽."
                                      generationBefore:@(generation)];
                           return;
                       }
                       [self requestJSONPath:@"/v1/local-saves"
                                      method:@"GET"
                                 bearerToken:managementToken
                                    jsonBody:nil
                              expectedStatus:200
                                  completion:^(id refreshedState, NSError *refreshError) {
                                    NSArray *refreshedSlots = [refreshedState isKindOfClass:NSDictionary.class]
                                        ? refreshedState[@"slots"]
                                        : nil;
                                    NSDictionary *refreshedSlot = [refreshedSlots isKindOfClass:NSArray.class]
                                        ? refreshedSlots.firstObject
                                        : nil;
                                    NSNumber *refreshedSlotID = [refreshedSlot isKindOfClass:NSDictionary.class]
                                        ? refreshedSlot[@"id"]
                                        : nil;
                                    if (refreshError != nil
                                        || ![refreshedSlotID isKindOfClass:NSNumber.class]) {
                                        [self failDiagnosticAtStage:@"management_features"
                                                          errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                                             status:@"失败: CN signup 后仍没有本地存档槽."
                                                   generationBefore:@(generation)];
                                        return;
                                    }
                                    [self runManagementFeatureRequestsFromGeneration:generation
                                                                                token:managementToken
                                                                               slotID:refreshedSlotID];
                                  }];
                     }];
                     return;
                 }
                 [self runManagementFeatureRequestsFromGeneration:generation
                                                             token:managementToken
                                                            slotID:slotID];
               }];
}

- (void)runManagementFeatureRequestsFromGeneration:(NSInteger)generation
                                             token:(NSString *)managementToken
                                            slotID:(NSNumber *)slotID {

                 NSDictionary *timeBody = @{
                     @"enabled" : @YES,
                     @"iso" : @"2030-01-01T12:00:00.000Z",
                     @"rate" : @1.0,
                 };
                 [self requestJSONPath:@"/v1/time"
                                method:@"PUT"
                           bearerToken:managementToken
                              jsonBody:timeBody
                        expectedStatus:200
                            completion:^(__unused id timeState, NSError *timeError) {
                              if (timeError != nil) {
                                  [self failDiagnosticAtStage:@"management_features"
                                                    errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                                       status:@"失败: iOS 内嵌服务无法设置虚拟日期."
                                             generationBefore:@(generation)];
                                  return;
                              }

                              NSString *mailPath = [NSString stringWithFormat:
                                  @"/v1/local-saves/%@/mails", slotID];
                              NSDictionary *mailBody = @{
                                  @"title" : [NSString stringWithFormat:@"iOS diagnostic %@", self.diagnosticRunID],
                                  @"body" : @"Local management smoke test",
                                  @"sender" : @"Starpoint",
                                  @"rewards" : @{
                                      @"freeVmoney" : @100,
                                      @"itemList" : @{ @"900001" : @1 },
                                  },
                              };
                              [self requestJSONPath:mailPath
                                             method:@"POST"
                                        bearerToken:managementToken
                                           jsonBody:mailBody
                                     expectedStatus:201
                                         completion:^(__unused id createdMail, NSError *mailError) {
                                           if (mailError != nil) {
                                               [self failDiagnosticAtStage:@"management_features"
                                                                 errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                                                    status:@"失败: iOS 内嵌服务无法按槽发放资源邮件."
                                                          generationBefore:@(generation)];
                                               return;
                                           }

                                           [self requestJSONPath:mailPath
                                                          method:@"GET"
                                                     bearerToken:managementToken
                                                        jsonBody:nil
                                                  expectedStatus:200
                                                      completion:^(id mails, NSError *mailListError) {
                                                        if (mailListError != nil
                                                            || ![mails isKindOfClass:NSArray.class]
                                                            || [(NSArray *)mails count] == 0) {
                                                            [self failDiagnosticAtStage:@"management_features"
                                                                              errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                                                                 status:@"失败: iOS 内嵌服务无法读取资源邮件."
                                                                       generationBefore:@(generation)];
                                                            return;
                                                        }

                                                        NSString *snapshotPath = [NSString stringWithFormat:
                                                            @"/v1/local-saves/%@/snapshots", slotID];
                                                        [self requestJSONPath:snapshotPath
                                                                       method:@"POST"
                                                                  bearerToken:managementToken
                                                                     jsonBody:@{ @"label" : @"iOS diagnostic" }
                                                               expectedStatus:201
                                                                   completion:^(__unused id snapshot, NSError *snapshotError) {
                                                                     if (snapshotError != nil) {
                                                                         [self failDiagnosticAtStage:@"management_features"
                                                                                           errorCode:@"MANAGEMENT_FEATURES_FAILED"
                                                                                              status:@"失败: iOS 内嵌服务无法创建存档快照."
                                                                                    generationBefore:@(generation)];
                                                                         return;
                                                                     }
                                                                     [self runPersistenceDiagnosticFromGeneration:generation];
                                                                   }];
                                                      }];
                                         }];
                             }];
}
// //// /验证 iOS 内嵌服务的存档, 时间和邮件管理 ////

- (void)runDeviceDiagnosticAttempt:(NSInteger)attempt {
    [self requestPath:@"/health"
               method:@"GET"
          bearerToken:nil
           completion:^(NSDictionary *health, NSError *healthError) {
             if (healthError != nil && attempt < 40) {
                 dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 250 * NSEC_PER_MSEC),
                                dispatch_get_main_queue(), ^{
                                  [self runDeviceDiagnosticAttempt:attempt + 1];
                                });
                 return;
             }
             if (healthError != nil) {
                 [self failDiagnosticAtStage:@"service_start"
                                   errorCode:@"SERVICE_START_TIMEOUT"
                                      status:@"失败: 个人服务未在 10 秒内启动."
                            generationBefore:nil];
                 return;
             }

             NSInteger generation = [health[@"generation"] integerValue];
             [self writeDiagnosticState:@"running"
                                  stage:@"management_auth"
                              errorCode:nil
                       generationBefore:@(generation)
                        generationAfter:nil];
             NSString *managementToken = [self managementToken];
             if (managementToken == nil) {
                 [self failDiagnosticAtStage:@"management_auth"
                                   errorCode:@"MANAGEMENT_AUTH_FAILED"
                                      status:@"失败: 原生宿主无法读取管理 token."
                            generationBefore:@(generation)];
                 return;
             }
             [self requestPath:@"/v1/server-profiles"
                          method:@"GET"
                     bearerToken:managementToken
                      completion:^(NSDictionary *profiles, NSError *profileError) {
                        if (profileError != nil || ![profiles[@"profiles"] isKindOfClass:NSArray.class]) {
                            [self failDiagnosticAtStage:@"management_auth"
                                              errorCode:@"MANAGEMENT_AUTH_FAILED"
                                                 status:@"失败: 管理 token 无法读取服务器配置."
                                       generationBefore:@(generation)];
                            return;
                        }
                        [self runManagementFeatureDiagnosticFromGeneration:generation
                                                                      token:managementToken];
                      }];
            }];
}

// //// 记录进入后台前的真实生命周期计数 [@x380kkm 2026-08-18] ////
- (void)recordLifecycleBaseline {
    self.backgroundFlushCountBefore =
        starpoint_personal_service_bootstrap_background_flush_count();
    self.foregroundResumeCountBefore =
        starpoint_personal_service_bootstrap_foreground_resume_count();
}
// //// /记录进入后台前的真实生命周期计数 ////

// //// 等待后台通知完成真实 SQLite 刷盘 [@x380kkm 2026-08-18] ////
- (void)waitForBackgroundCheckpointAttempt:(NSInteger)attempt {
    if (!self.lifecyclePending) {
        return;
    }

    uint64_t flushCount =
        starpoint_personal_service_bootstrap_background_flush_count();
    if (flushCount <= self.backgroundFlushCountBefore && attempt < 40) {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 250 * NSEC_PER_MSEC),
                       dispatch_get_main_queue(), ^{
                         [self waitForBackgroundCheckpointAttempt:attempt + 1];
                       });
        return;
    }

    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    int32_t flushResult =
        starpoint_personal_service_bootstrap_last_background_flush_result();
    [defaults setObject:@(flushCount) forKey:StarpointDiagnosticBackgroundFlushCountKey];
    [defaults setObject:@(flushResult) forKey:StarpointDiagnosticBackgroundFlushResultKey];
    NSNumber *generation = [defaults objectForKey:StarpointDiagnosticGenerationAfterKey];
    if (flushCount <= self.backgroundFlushCountBefore || flushResult != 0) {
        [self failDiagnosticAtStage:@"background_checkpoint"
                          errorCode:@"BACKGROUND_CHECKPOINT_FAILED"
                             status:@"失败: App 进入后台后 SQLite 刷盘未完成."
                   generationBefore:generation];
        return;
    }

    [defaults setObject:@"background_checkpoint_passed"
                 forKey:StarpointDiagnosticLifecycleStateKey];
    [self writeDiagnosticState:@"passed"
                         stage:@"background_checkpoint"
                     errorCode:nil
              generationBefore:generation
               generationAfter:generation];
}
// //// /等待后台通知完成真实 SQLite 刷盘 ////

// //// 验证后台刷盘和前台恢复的真实结果 [@x380kkm 2026-08-24] ////
- (void)runForegroundResumeDiagnostic {
    if (!self.lifecyclePending) {
        return;
    }

    [self requestPath:@"/health"
               method:@"GET"
           bearerToken:nil
            completion:^(NSDictionary *health, NSError *healthError) {
              if (healthError != nil) {
                  [self failDiagnosticAtStage:@"foreground_resume"
                                    errorCode:@"FOREGROUND_RESUME_FAILED"
                                       status:@"失败: App 回到前台后个人服务未恢复."
                             generationBefore:nil];
                  return;
              }

              uint64_t flushCount =
                  starpoint_personal_service_bootstrap_background_flush_count();
              int32_t flushResult =
                  starpoint_personal_service_bootstrap_last_background_flush_result();
              uint64_t resumeCount =
                  starpoint_personal_service_bootstrap_foreground_resume_count();
              NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
              [defaults setObject:@(flushCount)
                           forKey:StarpointDiagnosticBackgroundFlushCountKey];
              [defaults setObject:@(flushResult)
                           forKey:StarpointDiagnosticBackgroundFlushResultKey];
              [defaults setObject:@(resumeCount)
                           forKey:StarpointDiagnosticForegroundResumeCountKey];

              NSInteger generation = [health[@"generation"] integerValue];
              NSInteger baselineGeneration =
                  [[defaults objectForKey:StarpointDiagnosticGenerationAfterKey] integerValue];
              BOOL lifecycleCountersReady =
                  flushCount > self.backgroundFlushCountBefore &&
                  flushResult == 0 &&
                  resumeCount > self.foregroundResumeCountBefore;
              if (!lifecycleCountersReady) {
                  [self failDiagnosticAtStage:@"background_checkpoint"
                                    errorCode:@"BACKGROUND_CHECKPOINT_FAILED"
                                       status:@"失败: 未观察到真实后台刷盘和前台恢复计数."
                             generationBefore:@(baselineGeneration)];
                  return;
              }
              if (generation < baselineGeneration) {
                  [self failDiagnosticAtStage:@"foreground_resume"
                                   errorCode:@"PERSISTENCE_REGRESSION"
                                      status:@"失败: App 回到前台后 SQLite 代次回退."
                            generationBefore:@(baselineGeneration)];
                  return;
              }

              NSString *managementToken = [self managementToken];
              if (managementToken == nil) {
                  [self failDiagnosticAtStage:@"foreground_resume"
                                   errorCode:@"MANAGEMENT_AUTH_FAILED"
                                      status:@"失败: 前台恢复后无法读取管理 token."
                            generationBefore:@(baselineGeneration)];
                  return;
              }
              [self requestPath:@"/v1/server-profiles"
                          method:@"GET"
                     bearerToken:managementToken
                      completion:^(__unused NSDictionary *profiles, NSError *profileError) {
                        if (profileError != nil) {
                            [self failDiagnosticAtStage:@"foreground_resume"
                                              errorCode:@"MANAGEMENT_AUTH_FAILED"
                                                 status:@"失败: 前台恢复后管理鉴权不可用."
                                           generationBefore:@(baselineGeneration)];
                            return;
                        }
                        [defaults setObject:@"passed" forKey:StarpointDiagnosticLifecycleStateKey];
                        [self writeDiagnosticState:@"passed"
                                             stage:@"foreground_resume"
                                         errorCode:nil
                                  generationBefore:@(baselineGeneration)
                                   generationAfter:@(generation)];
                        [self reportStatus:@"通过: 后台刷盘、前台恢复和管理鉴权正常."
                                     color:UIColor.greenColor];
                        self.lifecyclePending = NO;
                      }];
           }];
}
// //// /验证后台刷盘和前台恢复的真实结果 ////

#if defined(STARPOINT_SELF_DIAGNOSTIC)
// //// 接收真实 UIApplication 生命周期通知 [@x380kkm 2026-08-24] ////
- (void)applicationWillResignActive:(__unused UIApplication *)application {
    if ([NSUserDefaults.standardUserDefaults objectForKey:StarpointDiagnosticGenerationKey] != nil) {
        [self recordLifecycleBaseline];
    }
}

- (void)applicationDidEnterBackground:(__unused UIApplication *)application {
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    if ([defaults objectForKey:StarpointDiagnosticGenerationKey] == nil) {
        return;
    }
    self.lifecyclePending = YES;
    [defaults setObject:@"backgrounded" forKey:StarpointDiagnosticLifecycleStateKey];
    NSNumber *baselineGeneration =
        [defaults objectForKey:StarpointDiagnosticGenerationAfterKey];
    [self writeDiagnosticState:@"running"
                         stage:@"background_checkpoint"
                     errorCode:nil
              generationBefore:baselineGeneration
               generationAfter:baselineGeneration];
    [self waitForBackgroundCheckpointAttempt:0];
}

- (void)applicationDidBecomeActive:(__unused UIApplication *)application {
    if (!self.lifecyclePending) {
        return;
    }
    [self runForegroundResumeDiagnostic];
}
// //// /接收真实 UIApplication 生命周期通知 ////
#endif

// //// 验证标题页管理按钮的幂等显示和彻底移除 [@x380kkm 2026-08-20] ////
- (NSUInteger)managementEntryButtonCount {
    NSUInteger count = 0;
    for (UIView *view in self.window.subviews) {
        if ([view.accessibilityIdentifier isEqualToString:StarpointManagementEntryAccessibilityIdentifier]) {
            count += 1;
        }
    }
    return count;
}

- (BOOL)verifyManagementEntryLifecycle {
    if (starpoint_personal_service_bootstrap_management_entry_visible() != 0 ||
        [self managementEntryButtonCount] != 0) {
        return NO;
    }

    starpoint_personal_service_bootstrap_set_management_entry_visible(1);
    starpoint_personal_service_bootstrap_set_management_entry_visible(1);
    if (starpoint_personal_service_bootstrap_management_entry_visible() != 1 ||
        [self managementEntryButtonCount] != 1) {
        return NO;
    }

    starpoint_personal_service_bootstrap_set_management_entry_visible(0);
    return starpoint_personal_service_bootstrap_management_entry_visible() == 0 &&
           [self managementEntryButtonCount] == 0;
}
// //// /验证标题页管理按钮的幂等显示和彻底移除 ////

- (BOOL)application:(__unused UIApplication *)application
    didFinishLaunchingWithOptions:(__unused NSDictionary<UIApplicationLaunchOptionsKey, id> *)launchOptions {
    starpoint_personal_service_bootstrap_link();
    starpoint_personal_service_bootstrap_start();
    self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
    UIViewController *controller = [[UIViewController alloc] init];
    controller.view.backgroundColor = UIColor.whiteColor;
    self.statusLabel = [[UILabel alloc] initWithFrame:CGRectMake(
        24.0,
        72.0,
        CGRectGetWidth(controller.view.bounds) - 48.0,
        CGRectGetHeight(controller.view.bounds) - 196.0
    )];
    self.statusLabel.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    self.statusLabel.numberOfLines = 0;
    self.statusLabel.textAlignment = NSTextAlignmentCenter;
#if defined(STARPOINT_SELF_DIAGNOSTIC)
    self.statusLabel.text = @"正在检查个人服务...";
#else
    self.statusLabel.text = @"Simulator 自动化测试正在外部检查个人服务.";
#endif
    [controller.view addSubview:self.statusLabel];
    self.managementButton = [UIButton buttonWithType:UIButtonTypeSystem];
    self.managementButton.frame = CGRectMake(
        24.0,
        CGRectGetHeight(controller.view.bounds) - 92.0,
        CGRectGetWidth(controller.view.bounds) - 48.0,
        52.0
    );
    self.managementButton.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleTopMargin;
    self.managementButton.enabled = NO;
    self.managementButton.backgroundColor = [UIColor colorWithRed:0.05 green:0.46 blue:0.43 alpha:1.0];
    self.managementButton.tintColor = UIColor.whiteColor;
    self.managementButton.layer.cornerRadius = 14.0;
    [self.managementButton setTitle:@"打开本地管理界面" forState:UIControlStateNormal];
    [self.managementButton addTarget:self
                              action:@selector(openManagement)
                    forControlEvents:UIControlEventTouchUpInside];
    [controller.view addSubview:self.managementButton];
    self.window.rootViewController = controller;
    [self.window makeKeyAndVisible];
    self.diagnosticRunID = NSUUID.UUID.UUIDString;
    if (![self verifyManagementEntryLifecycle]) {
        [self failDiagnosticAtStage:@"management_entry"
                          errorCode:@"MANAGEMENT_ENTRY_FAILED"
                             status:@"失败: 标题页管理按钮生命周期异常."
                   generationBefore:nil];
        return YES;
    }
    [self waitForManagementPageAttempt:0];
#if defined(STARPOINT_SELF_DIAGNOSTIC)
    [self writeDiagnosticState:@"running"
                         stage:@"service_start"
                     errorCode:nil
              generationBefore:nil
               generationAfter:nil];
    [self runDeviceDiagnosticAttempt:0];
#endif
    return YES;
}

@end
// //// /运行 iOS 个人服务诊断 ////

// //// 进入 UIKit 应用事件循环 [@x380kkm 2026-07-23] ////
int main(int argc, char *argv[]) {
    @autoreleasepool {
        return UIApplicationMain(argc, argv, nil, NSStringFromClass(StarpointDiagnosticAppDelegate.class));
    }
}
// //// /进入 UIKit 应用事件循环 ////
