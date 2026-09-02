# audience: internal
# # ios-personal-service-bootstrap-contract
# 此测试不修改工作树, 在 Windows 上验证 iOS Framework 管理入口和归档行尾契约.

from __future__ import annotations

import io
import subprocess
import sys
import tarfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
IOS_BOOTSTRAP = (
    REPOSITORY_ROOT
    / "platforms"
    / "ios"
    / "PersonalServiceBootstrap"
    / "StarpointPersonalServiceBootstrap.m"
)
IOS_BUILD_SCRIPT = REPOSITORY_ROOT / "platforms" / "ios" / "build-framework.sh"
IOS_DEVICE_HARNESS = REPOSITORY_ROOT / "platforms" / "ios" / "build-device-harness.sh"
IOS_SIMULATOR_HARNESS = (
    REPOSITORY_ROOT / "platforms" / "ios" / "build-simulator-harness.sh"
)
IOS_SIMULATOR_DIAGNOSTIC = (
    REPOSITORY_ROOT / "platforms" / "ios" / "run-simulator-diagnostic.sh"
)
IOS_SIMULATOR_DIAGNOSTIC_LIBRARY = (
    REPOSITORY_ROOT / "platforms" / "ios" / "ios-simulator-diagnostic-lib.sh"
)
IOS_VALIDATION_CLEANUP = (
    REPOSITORY_ROOT / "platforms" / "ios" / "cleanup-ios-validation.sh"
)
IOS_VALIDATION_PROCESS_STOP = (
    REPOSITORY_ROOT / "platforms" / "ios" / "stop-ios-validation-process.sh"
)
IOS_DIAGNOSTIC_SANITIZER = (
    REPOSITORY_ROOT / "platforms" / "ios" / "sanitize-diagnostic-detail.py"
)
IOS_DIAGNOSTIC = REPOSITORY_ROOT / "platforms" / "ios" / "DiagnosticHarness" / "main.m"
IOS_CN_PACKAGE_SCRIPT = (
    REPOSITORY_ROOT / "scripts" / "protocol-lab" / "package-ios-cn-personal-service.ps1"
)
IOS_PACKAGE_MODULE = (
    REPOSITORY_ROOT / "scripts" / "protocol-lab" / "package_ios_personal_service.py"
)


# //// 读取 Git 索引并生成待提交归档树 [@x380kkm 2026-08-17] ////
def run_git(*arguments: str) -> bytes:
    result = subprocess.run(
        ["git", "-C", str(REPOSITORY_ROOT), *arguments],
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        error = result.stderr.decode("utf-8", errors="replace").strip()
        raise AssertionError(f"git {' '.join(arguments)} 执行失败: {error}")
    return result.stdout


def get_tracked_shell_script_paths() -> list[str]:
    return [
        path.decode("utf-8")
        for path in run_git("ls-files", "-z", "--", "*.sh").split(b"\0")
        if path
    ]


def write_staged_tree() -> str:
    treeish = run_git("write-tree").decode("ascii").strip()
    if not treeish:
        raise AssertionError("Git 未生成待提交树.")
    return treeish


# //// /读取 Git 索引并生成待提交归档树 ////


# //// 验证 iOS 管理入口, Framework 链接和归档行尾契约 [@x380kkm 2026-08-25] ////
class IosPersonalServiceBootstrapContractTest(unittest.TestCase):
    def test_routes_cn_login_hosts_and_failed_loopback_tls(self) -> None:
        source = IOS_BOOTSTRAP.read_text(encoding="utf-8")
        routing = source[
            source.index("static BOOL isCnLoginSdkHost") : source.index(
                "// //// /改写 CN 原生登录请求地址 ////"
            )
        ]

        for host_match in (
            '[normalizedHost isEqualToString:@"leiting.com"]',
            '[normalizedHost hasSuffix:@".leiting.com"]',
            '[normalizedHost isEqualToString:@"roguelike.com"]',
            '[normalizedHost hasSuffix:@".roguelike.com"]',
            '[normalizedHost isEqualToString:@"cl2009.com"]',
            '[normalizedHost hasSuffix:@".cl2009.com"]',
        ):
            self.assertIn(host_match, routing)
        self.assertIn(
            '[components.scheme.lowercaseString isEqualToString:@"https"]', routing
        )
        self.assertIn(
            '[components.host.lowercaseString isEqualToString:@"127.0.0.1"]',
            routing,
        )
        self.assertIn("port == nil || port.unsignedIntegerValue == 443", routing)
        self.assertIn('components.scheme = @"http";', routing)
        self.assertIn('components.host = @"127.0.0.1";', routing)
        self.assertIn("components.port = @(StarpointPersonalServicePort);", routing)
        for external_sdk_host in (
            "127.1",
            "www.ip138.com",
            "pv.sohu.com",
            "wpa.qq.com",
            "api.sobot.com",
            "img.sobot.com",
            "www.sobot.com",
        ):
            self.assertNotIn(external_sdk_host, routing)

    def test_native_login_routing_preserves_the_request_and_cannot_recurse(self) -> None:
        source = IOS_BOOTSTRAP.read_text(encoding="utf-8")

        request_routing = source[
            source.index("static NSURLRequest *routeCnLoginSdkRequest") : source.index(
                "@interface NSURLSession (StarpointCnLoginRouting)"
            )
        ]
        session_routing = source[
            source.index("@implementation NSURLSession (StarpointCnLoginRouting)") : source.index(
                "// //// /改写 CN 原生登录请求地址 ////"
            )
        ]

        self.assertIn(
            "NSMutableURLRequest *routedRequest = [request mutableCopy];",
            request_routing,
        )
        self.assertIn("routedRequest.URL = routedURL;", source)
        self.assertEqual(1, request_routing.count("routedRequest."))
        self.assertEqual(4, session_routing.count("[self starpoint_dataTask"))
        self.assertEqual(4, session_routing.count("return [self starpoint_dataTask"))
        self.assertNotIn("schedulePersonalServiceForegroundRecovery", session_routing)
        self.assertIn("method_exchangeImplementations(originalMethod, routingMethod)", source)
        for selector in (
            "dataTaskWithRequest:",
            "dataTaskWithRequest:completionHandler:",
            "dataTaskWithURL:",
            "dataTaskWithURL:completionHandler:",
        ):
            self.assertIn(f"@selector({selector})", source)
            self.assertIn(f"@selector(starpoint_{selector})", source)
        self.assertLess(
            source.index("installNativeLoginURLRouting();"),
            source.index('dispatch_queue_create("dev.starpoint.personal-service"'),
        )

    def test_native_requests_start_immediately(self) -> None:
        source = IOS_BOOTSTRAP.read_text(encoding="utf-8")

        self.assertNotIn("StarpointPersonalServiceRequestBarrier", source)
        self.assertNotIn("guardPersonalServiceDataTask", source)
        self.assertNotIn("starpoint_resumeAfterPersonalServiceReadiness", source)
        self.assertNotIn("@selector(resume)", source)

    def test_embedded_service_recovers_across_bounded_lifecycle_transitions(self) -> None:
        source = IOS_BOOTSTRAP.read_text(encoding="utf-8")
        discard = source[
            source.index("static void discardPersonalServiceOnQueue(void)") : source.index(
                "// //// /从串行队列移除失效的个人服务 ////"
            )
        ]
        health_probe = source[
            source.index("static BOOL waitForPersonalServiceSocketEvent") : source.index(
                "// //// /探测 loopback 个人服务的 HTTP 健康状态 ////"
            )
        ]
        readiness = source[
            source.index("static BOOL ensurePersonalServiceReadyOnQueue(void)") : source.index(
                "// //// /等待个人服务进入可用状态 ////"
            )
        ]
        startup_ensure = source[
            source.index("static BOOL ensurePersonalServiceReadyOnQueue(void)") : source.index(
                "// //// /在串行队列中确保个人服务可用 ////"
            )
        ]
        lifecycle = source[
            source.index("static void endPersonalServiceBackgroundExecution") : source.index(
                "// //// /绑定 App 生命周期通知 ////"
            )
        ]

        recovery = source[
            source.index("static BOOL recoverPersonalServiceForForegroundOnQueue") : source.index(
                "// //// /恢复前台个人服务 ////"
            )
        ]
        scheduling = source[
            source.index("static BOOL isPersonalServiceForegroundRecoveryCurrent") : source.index(
                "// //// /在恢复窗口内排队前台个人服务恢复 ////"
            )
        ]
        recovery_attempt_start = source.index(
            "static void attemptPersonalServiceForegroundRecoveryOnQueue"
        )
        recovery_attempt = source[
            recovery_attempt_start : source.index(
                "static void schedulePersonalServiceForegroundRecovery",
                recovery_attempt_start,
            )
        ]
        activation = source[
            source.index(
                "static void schedulePersonalServiceForegroundActivationCompletion"
            ) : source.index(
                "// //// /排队前台激活收尾 ////"
            )
        ]
        lifecycle_notifications = source[
            source.index("UIApplicationWillEnterForegroundNotification") : source.index(
                "UIApplicationWillTerminateNotification"
            )
        ]

        self.assertNotIn("dispatch_semaphore_wait(", source)
        self.assertIn("StarpointPersonalServiceHealthProbeTimeoutMilliseconds", health_probe)
        self.assertIn("poll(", health_probe)
        self.assertIn("htons(StarpointPersonalServicePort)", health_probe)
        self.assertIn('"GET /health HTTP/1.1\\r\\n"', health_probe)
        self.assertIn('static const char healthyStatus[] = "HTTP/1.1 200";', health_probe)
        self.assertIn("StarpointPersonalServiceHealthListening", health_probe)
        self.assertIn("StarpointPersonalServiceHealthReady", health_probe)
        self.assertIn("probePersonalServiceHealthOnQueue()", recovery)
        self.assertIn(
            "starpoint_personal_service_is_running(personalService) != 0", startup_ensure
        )
        self.assertIn("discardPersonalServiceOnQueue();", startup_ensure)
        self.assertIn("discardPersonalServiceOnQueue();", recovery)
        self.assertLess(
            recovery.index("discardPersonalServiceOnQueue();"),
            recovery.index("ensurePersonalServiceReadyOnQueue()"),
        )
        self.assertLess(
            discard.index("personalService = NULL;"),
            discard.index("starpoint_personal_service_stop(discardedPersonalService);"),
        )
        self.assertIn("dispatch_sync(personalServiceQueue", readiness)
        self.assertIn("StarpointForegroundRecoveryMaximumAttempts", scheduling)
        self.assertIn("StarpointForegroundRecoveryRestartInterval", recovery)
        self.assertIn("healthState == StarpointPersonalServiceHealthUnreachable", recovery)
        self.assertIn("attempt > 0", recovery)
        self.assertIn("&foregroundRecoveryScheduled", scheduling)
        self.assertIn("foregroundRecoveryCompletions", scheduling)
        self.assertIn("completePersonalServiceForegroundRecoveryOnQueue", scheduling)
        self.assertIn("dispatch_after(", scheduling)
        self.assertEqual(
            2,
            recovery_attempt.count(
                "completePersonalServiceForegroundRecoveryOnQueue(generation,"
            ),
        )
        self.assertIn("atomic_store_explicit(", scheduling)
        self.assertIn("attemptPersonalServiceForegroundRecoveryOnQueue(0, generation);", scheduling)
        self.assertIn("schedulePersonalServiceForegroundRecovery(^(", activation)
        self.assertIn("endPersonalServiceBackgroundExecution", activation)
        self.assertNotIn("dispatch_sync", activation)
        self.assertNotIn("dispatch_get_specific", source)
        self.assertIn("UIApplicationWillResignActiveNotification", lifecycle)
        self.assertIn("UIApplicationDidEnterBackgroundNotification", lifecycle)
        self.assertIn("UIApplicationWillEnterForegroundNotification", lifecycle)
        self.assertIn("beginBackgroundTaskWithName", lifecycle)
        self.assertIn("endBackgroundTask:task", lifecycle)
        self.assertIn("flushPersonalServiceForBackground(application, generation);", lifecycle)
        self.assertIn("cancelPersonalServiceForegroundRecovery();", lifecycle)
        self.assertIn("schedulePersonalServiceForegroundRecovery(nil);", lifecycle_notifications)
        self.assertIn(
            "schedulePersonalServiceForegroundActivationCompletion(",
            lifecycle_notifications,
        )
        self.assertNotIn("ensurePersonalServiceReady();", lifecycle_notifications)

        diagnostic = IOS_DIAGNOSTIC.read_text(encoding="utf-8")
        foreground_diagnostic = diagnostic[
            diagnostic.index("- (void)runForegroundResumeDiagnostic") : diagnostic.index(
                "// //// /验证后台刷盘和前台恢复的真实结果 ////"
            )
        ]
        self.assertNotIn("dispatch_after(", foreground_diagnostic)
        self.assertNotIn("Attempt:", foreground_diagnostic)
        self.assertIn("[self runForegroundResumeDiagnostic];", diagnostic)

    def test_reconciles_the_title_scene_management_button_on_activation(self) -> None:
        source = IOS_BOOTSTRAP.read_text(encoding="utf-8")

        self.assertIn("UIApplicationDidBecomeActiveNotification", source)
        self.assertIn("if (@available(iOS 13.0, *))", source)
        self.assertIn("application.connectedScenes", source)
        self.assertIn("UIWindowScene", source)
        self.assertIn("application.keyWindow", source)
        self.assertLess(
            source.index("application.connectedScenes"), source.index("delegate.window")
        )
        self.assertIn("starpoint_personal_service_bootstrap_set_management_entry_visible", source)
        self.assertIn('accessibilityIdentifier = @"starpoint.management.entry"', source)
        self.assertIn("reconcileManagementEntry(application);", source)
        self.assertIn("UIControlEventTouchUpInside", source)

    def test_presents_the_local_manage_page_from_the_title_button(self) -> None:
        source = IOS_BOOTSTRAP.read_text(encoding="utf-8")

        self.assertIn("#import <SafariServices/SafariServices.h>", source)
        self.assertIn("SFSafariViewController", source)
        self.assertIn("topViewController(window.rootViewController)", source)
        self.assertIn('components.host = @"127.0.0.1";', source)
        self.assertIn("components.port = @(starpoint_personal_service_bootstrap_port());", source)
        self.assertIn('components.path = @"/manage/";', source)
        self.assertIn("presentManagementPage();", source)
        self.assertIn("[presenter presentViewController:controller", source)

    def test_links_safari_services_for_the_ios12_build_target(self) -> None:
        script = IOS_BUILD_SCRIPT.read_text(encoding="utf-8")

        self.assertIn('MINIMUM_IOS_VERSION="12.0"', script)
        self.assertIn('export IPHONEOS_DEPLOYMENT_TARGET="$MINIMUM_IOS_VERSION"', script)
        self.assertIn(
            'CARGO_TARGET_ROOT="${CARGO_TARGET_DIR:-$CORE_ROOT/target}"', script
        )
        self.assertIn("-framework SafariServices", script)

    def test_device_and_simulator_enable_the_same_machine_readable_diagnostic(self) -> None:
        diagnostic = IOS_DIAGNOSTIC.read_text(encoding="utf-8")
        device_script = IOS_DEVICE_HARNESS.read_text(encoding="utf-8")
        simulator_script = IOS_SIMULATOR_HARNESS.read_text(encoding="utf-8")

        self.assertIn("STARPOINT_SELF_DIAGNOSTIC", device_script)
        self.assertIn("STARPOINT_SELF_DIAGNOSTIC", simulator_script)
        self.assertNotIn("STARPOINT_DEVICE_DIAGNOSTIC", diagnostic)
        for key in (
            'StarpointDiagnosticRunIDKey = @"run_id"',
            'StarpointDiagnosticStateKey = @"state"',
            'StarpointDiagnosticStageKey = @"stage"',
            'StarpointDiagnosticErrorCodeKey = @"error_code"',
            'StarpointDiagnosticGenerationBeforeKey = @"generation_before"',
            'StarpointDiagnosticGenerationAfterKey = @"generation_after"',
            'StarpointDiagnosticLifecycleStateKey = @"lifecycle_state"',
            'StarpointDiagnosticBackgroundFlushCountKey = @"background_flush_count"',
            'StarpointDiagnosticBackgroundFlushResultKey = @"background_flush_result"',
            'StarpointDiagnosticForegroundResumeCountKey = @"foreground_resume_count"',
        ):
            self.assertIn(key, diagnostic)
        for state in ('@"running"', '@"passed"', '@"failed"'):
            self.assertIn(state, diagnostic)
        for stage in (
            '@"service_start"',
            '@"management_auth"',
            '@"management_features"',
            '@"state_increment"',
            '@"checkpoint"',
            '@"complete"',
            '@"background_checkpoint"',
            '@"foreground_resume"',
        ):
            self.assertIn(stage, diagnostic)
        for feature in (
            "requestJSONPath",
            "requestDiagnosticSignup",
            "galkZXZpY2VfaWQB",
            "expectedStatus",
            '"/v1/local-saves"',
            '"/v1/time"',
            "/mails",
            "/snapshots",
            '"MANAGEMENT_FEATURES_FAILED"',
        ):
            self.assertIn(feature, diagnostic)
        self.assertIn("applicationDidEnterBackground", diagnostic)
        self.assertIn("applicationDidBecomeActive", diagnostic)
        bootstrap = IOS_BOOTSTRAP.read_text(encoding="utf-8")
        self.assertIn("starpoint_personal_service_bootstrap_background_flush_count", bootstrap)
        self.assertIn("starpoint_personal_service_bootstrap_last_background_flush_result", bootstrap)
        self.assertIn("starpoint_personal_service_bootstrap_foreground_resume_count", bootstrap)
        self.assertIn("atomic_fetch_add_explicit", bootstrap)
        simulator_diagnostic = IOS_SIMULATOR_DIAGNOSTIC.read_text(encoding="utf-8")
        simulator_library = IOS_SIMULATOR_DIAGNOSTIC_LIBRARY.read_text(encoding="utf-8")
        cleanup = IOS_VALIDATION_CLEANUP.read_text(encoding="utf-8")
        process_stop = IOS_VALIDATION_PROCESS_STOP.read_text(encoding="utf-8")
        self.assertIn("com.apple.CoreSimulator.SimRuntime.iOS-26-5", simulator_diagnostic)
        self.assertIn('ios-simulator-diagnostic-lib.sh"', simulator_diagnostic)
        self.assertIn("write_report()", simulator_library)
        self.assertIn("has_adhoc_signature()", simulator_library)
        self.assertIn(
            'management_features) printf \'%s\\n\' "MANAGEMENT_FEATURES_FAILED"',
            simulator_library,
        )
        self.assertIn('["plutil", "-convert", "json", "-o", "-",', simulator_library)
        self.assertNotIn('grep -Fq "Signature=adhoc"', simulator_diagnostic)
        self.assertIn("simulator-udid.txt", cleanup)
        self.assertIn("stop-ios-validation-process.sh", cleanup)
        self.assertIn("device|simulator", process_stop)
        self.assertIn("collect_reparented_build_processes()", process_stop)
        self.assertGreaterEqual(process_stop.count("collect_validation_processes"), 3)
        self.assertIn("signal_validation_processes KILL", process_stop)
        self.assertIn("sleep 0.25", process_stop)
        self.assertNotIn("shutdown all", simulator_diagnostic)
        self.assertNotIn("shutdown all", cleanup)

        secret_text = (
            "discarded-prefix "
            + "x" * 900
            + " Authorization: Bearer auth-value token=token-value "
            + '"password":"password-value" password="alpha beta gamma" '
            + r'"password":"alpha\"beta gamma" '
            + r"secret='delta\'epsilon zeta' "
            + "https://user:password@example.invalid/path "
            + "-----BEGIN PRIVATE KEY-----private-value-----END PRIVATE KEY----- "
            + "private_key=-----BEGIN PRIVATE KEY-----nested-value-----END PRIVATE KEY----- "
            + "tail-marker"
        )
        sanitized = subprocess.run(
            [sys.executable, str(IOS_DIAGNOSTIC_SANITIZER)],
            input=secret_text,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        self.assertLessEqual(len(sanitized), 800)
        self.assertTrue(sanitized.endswith("tail-marker"))
        self.assertNotIn("discarded-prefix", sanitized)
        for secret in (
            "auth-value",
            "token-value",
            "password-value",
            "alpha beta gamma",
            "beta gamma",
            "epsilon zeta",
            "user:password",
            "private-value",
            "nested-value",
        ):
            self.assertNotIn(secret, sanitized)

        truncated_pem = subprocess.run(
            [sys.executable, str(IOS_DIAGNOSTIC_SANITIZER)],
            input="prefix -----BEGIN PRIVATE KEY-----truncated-value",
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        self.assertEqual("prefix [redacted-pem]", truncated_pem)

    def test_cn_cdn_bootstrap_is_isolated_atomic_and_observable(self) -> None:
        bootstrap = IOS_BOOTSTRAP.read_text(encoding="utf-8")
        header = (
            REPOSITORY_ROOT
            / "platforms"
            / "ios"
            / "PersonalServiceBootstrap"
            / "StarpointPersonalServiceBootstrap.h"
        ).read_text(encoding="utf-8")
        for marker in (
            '@"STARPOINT_CN_CDN_BUNDLE_PATH"',
            '@"StarpointCNCDNBundlePath"',
            "URLByResolvingSymlinksInPath",
            "isDescendantURL",
            "NSURLIsSymbolicLinkKey",
            "NSURLIsRegularFileKey",
            "copyItemAtURL:sourceURL",
            "replaceItemAtURL:targetURL",
            "moveItemAtURL:stagingURL",
            "StarpointCnCdnImportMarkerName",
            "StarpointCnCdnBundleModeDirect",
            "configuredCnCdnBundleMode",
            "starpoint_personal_service_start_with_cdn_root",
            "STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_EMPTY",
            "starpoint_personal_service_bootstrap_cdn_import_count",
            "starpoint_personal_service_bootstrap_last_start_result",
        ):
            self.assertIn(marker, bootstrap)
        self.assertIn(
            "STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_BUNDLE_EMPTY",
            header,
        )
        self.assertIn(
            "STARPOINT_PERSONAL_SERVICE_BOOTSTRAP_START_CDN_IMPORT_FAILED",
            header,
        )

    def test_cn_packaging_requires_and_embeds_a_non_empty_cdn_bundle(self) -> None:
        package_script = IOS_CN_PACKAGE_SCRIPT.read_text(encoding="utf-8")
        package_module = IOS_PACKAGE_MODULE.read_text(encoding="utf-8")
        self.assertIn("[Parameter(Mandatory)][string]$CnCdnBundle", package_script)
        self.assertIn("'--cn-cdn-bundle', $cnCdnBundlePath", package_script)
        self.assertIn("StarpointCNCDN", package_module)
        self.assertIn("StarpointCNCDNBundleMode", package_module)
        self.assertIn('"direct"', package_module)
        self.assertIn("validate_cn_cdn_bundle", package_module)
        self.assertIn("cn_cdn_file_count", package_module)

    def test_cn_packaging_rebuilds_stale_voice_reports_and_checks_archive_identity(self) -> None:
        package_script = IOS_CN_PACKAGE_SCRIPT.read_text(encoding="utf-8")
        for marker in (
            "$expectedCnVoiceRoleCount = 17",
            "$expectedCnVoiceEntryCount = 325",
            "Get-CnVoiceOverlaySourceFingerprint",
            "Read-CnVoiceOverlayReportIfCurrent",
            "source_fingerprint",
            "[Security.Cryptography.SHA256]::HashData",
            "[IO.Compression.ZipFile]::OpenRead",
            "$installedArchiveMatchesReport",
            "archive.sha256",
            "report_reused",
        ):
            self.assertIn(marker, package_script)

    def test_cn_packaging_projects_all_gacha_entries_before_visual_overlays(self) -> None:
        package_script = IOS_CN_PACKAGE_SCRIPT.read_text(encoding="utf-8")
        master_call = package_script.index("$gachaMasterDiff = Update-CnGachaMasterDiff")
        voice_call = package_script.index("$voiceOverlay = Update-CnVoiceOverlay")
        banner_call = package_script.index("$gachaVisual = Update-CnGachaBanners")
        self.assertLess(master_call, voice_call)
        self.assertLess(voice_call, banner_call)
        for marker in (
            "ios-1.4.54-initial",
            "Initialize-CnActivityCatalog",
            "catalog_activity_count",
            "catalog_gacha_activity_count",
            "catalog_daily_activity_count",
            "retained_original_count",
            "temporary_alias_count",
            "gacha_count",
            "gacha_campaign_count",
            "projected_feature_link_count",
            "cn_gacha_master_diff",
        ):
            self.assertIn(marker, package_script)

    def test_validation_shell_scripts_use_lf_in_the_worktree(self) -> None:
        for path in (
            IOS_SIMULATOR_DIAGNOSTIC,
            IOS_SIMULATOR_DIAGNOSTIC_LIBRARY,
            IOS_VALIDATION_CLEANUP,
            IOS_VALIDATION_PROCESS_STOP,
        ):
            self.assertNotIn(b"\r\n", path.read_bytes(), f"{path} must use LF line endings.")

    def test_all_tracked_shell_scripts_use_lf_in_git_archives(self) -> None:
        shell_script_paths = get_tracked_shell_script_paths()
        self.assertTrue(shell_script_paths, "Git 索引没有已跟踪的 shell 脚本.")

        for shell_script_path in shell_script_paths:
            with self.subTest(path=shell_script_path):
                attribute_parts = [
                    part.decode("utf-8")
                    for part in run_git(
                        "check-attr",
                        "--cached",
                        "-z",
                        "text",
                        "eol",
                        "--",
                        shell_script_path,
                    ).split(b"\0")
                    if part
                ]
                self.assertEqual(
                    [
                        shell_script_path,
                        "text",
                        "set",
                        shell_script_path,
                        "eol",
                        "lf",
                    ],
                    attribute_parts,
                    f"{shell_script_path} 未应用 text eol=lf Git 属性.",
                )

                self.assertNotIn(
                    b"\r",
                    run_git("show", f":{shell_script_path}"),
                    f"{shell_script_path} 的 Git 索引内容包含 CR 字节.",
                )

        archive_bytes = run_git(
            "archive",
            "--format=tar",
            write_staged_tree(),
            "--",
            *shell_script_paths,
        )
        with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:") as archive:
            archived_shell_script_paths = {
                member.name for member in archive.getmembers() if member.isfile()
            }
            self.assertEqual(
                set(shell_script_paths),
                archived_shell_script_paths,
                "Git archive 没有包含全部已跟踪的 shell 脚本.",
            )

            for shell_script_path in shell_script_paths:
                with self.subTest(archive_path=shell_script_path):
                    archived_script = archive.extractfile(shell_script_path)
                    if archived_script is None:
                        self.fail(f"Git archive 无法读取 {shell_script_path}.")
                    self.assertNotIn(
                        b"\r",
                        archived_script.read(),
                        f"{shell_script_path} 的 Git archive 内容包含 CR 字节.",
                    )


# //// /验证 iOS 管理入口, Framework 链接和归档行尾契约 ////


if __name__ == "__main__":
    unittest.main()
