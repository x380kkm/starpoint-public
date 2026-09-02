// audience: internal
// # gbits-version-query-probe-patch-test
// 此测试使用外部 CN SWF 验证版本查询补丁只修改 isQuerySuccess.
// 运行前提是 Java 17, Starview 随附的 FFDec 库和未提交的 CN 客户端 SWF.

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Set;

public final class GbitsVersionQueryProbePatchTest {
    private static final String PATCH_METHOD_NAME = "isQuerySuccess";

    // //// 阻止实例化版本查询补丁测试 [@x380kkm 2026-08-12] ////
    private GbitsVersionQueryProbePatchTest() {
    }
    // //// /阻止实例化版本查询补丁测试 ////

    // //// 运行真实 SWF 回归测试 [@x380kkm 2026-08-12] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: GbitsVersionQueryProbePatchTest "
                            + "<reference.swf> <temporary-directory>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path temporaryDirectory = Path.of(args[1]).toAbsolutePath().normalize();
        Files.createDirectories(temporaryDirectory);
        Path inputPath = temporaryDirectory.resolve("gbits-version-query-input.swf");
        Path outputPath = temporaryDirectory.resolve("gbits-version-query-probe.swf");
        Path evidencePath = temporaryDirectory.resolve("gbits-version-query-probe-evidence.json");
        Path repeatedOutputPath = temporaryDirectory.resolve("gbits-version-query-probe-repeated.swf");
        Path repeatedEvidencePath = temporaryDirectory.resolve("gbits-version-query-probe-repeated-evidence.json");
        deleteOutputs(inputPath, outputPath, evidencePath, repeatedOutputPath, repeatedEvidencePath);
        Files.copy(referencePath, inputPath);

        Map<String, String> referenceDigests =
                GbitsVersionQueryProbePatch.digestVersionLogicMethods(referencePath);
        GbitsVersionQueryProbePatch.main(new String[] {
                referencePath.toString(),
                inputPath.toString(),
                outputPath.toString(),
                evidencePath.toString()
        });

        Map<String, String> outputDigests =
                GbitsVersionQueryProbePatch.digestVersionLogicMethods(outputPath);
        verifyMethodChanges(referenceDigests, outputDigests);
        verifyEvidence(evidencePath, inputPath, referencePath, outputPath, referenceDigests, outputDigests);
        verifyRepeatedApplicationRejected(
                referencePath,
                outputPath,
                outputPath,
                repeatedOutputPath,
                repeatedEvidencePath);
        System.out.println("GbitsVersionQueryProbePatchTest: PASS");
    }
    // //// /运行真实 SWF 回归测试 ////

    // //// 删除测试输出 [@x380kkm 2026-08-12] ////
    private static void deleteOutputs(Path... paths) throws Exception {
        for (Path path : paths) {
            Files.deleteIfExists(path);
            Files.deleteIfExists(path.resolveSibling(path.getFileName() + ".tmp"));
        }
    }
    // //// /删除测试输出 ////

    // //// 验证只有 isQuerySuccess 实例方法变化 [@x380kkm 2026-08-12] ////
    private static void verifyMethodChanges(
            Map<String, String> referenceDigests,
            Map<String, String> outputDigests) {
        Set<String> changedMethods = AbcMethodDigest.changedMethods(referenceDigests, outputDigests);
        if (!changedMethods.equals(Set.of(PATCH_METHOD_NAME))) {
            throw new IllegalStateException(
                    "unexpected GbitsVersionLogic method changes: " + changedMethods);
        }
        for (String methodName : referenceDigests.keySet()) {
            boolean changed = !referenceDigests.get(methodName).equals(outputDigests.get(methodName));
            if (methodName.equals(PATCH_METHOD_NAME) != changed) {
                throw new IllegalStateException(
                        "GbitsVersionLogic method change mismatch: " + methodName);
            }
        }
    }
    // //// /验证只有 isQuerySuccess 实例方法变化 ////

    // //// 验证完整性证据字段和摘要 [@x380kkm 2026-08-12] ////
    private static void verifyEvidence(
            Path evidencePath,
            Path inputPath,
            Path referencePath,
            Path outputPath,
            Map<String, String> referenceDigests,
            Map<String, String> outputDigests) throws Exception {
        String evidence = Files.readString(evidencePath, StandardCharsets.UTF_8);
        requireContains(evidence, "\"className\": \"pinball.gbits.logic.GbitsVersionLogic\"");
        requireContains(evidence, "\"patchMethod\": \"isQuerySuccess\"");
        requireContains(evidence, "\"digestVersion\": " + AbcMethodDigest.VERSION);
        requireContains(evidence, "\"methodOnly\": true");
        requireContains(evidence, "\"removesSdkDummyEarlySuccess\": true");
        requireContains(evidence, "\"preservesPublishTargetCheck\": true");
        requireContains(evidence, "\"preservesVersionsCheck\": true");
        requireContains(evidence, "\"changedMethods\": [\"isQuerySuccess\"]");
        requireContains(
                evidence,
                "\"referenceSha256\": \"" + referenceDigests.get(PATCH_METHOD_NAME) + "\"");
        requireContains(
                evidence,
                "\"inputSha256\": \"" + referenceDigests.get(PATCH_METHOD_NAME) + "\"");
        requireContains(
                evidence,
                "\"outputSha256\": \"" + outputDigests.get(PATCH_METHOD_NAME) + "\"");
        if (evidence.contains(referencePath.toString())
                || evidence.contains(inputPath.toString())
                || evidence.contains(outputPath.toString())) {
            throw new IllegalStateException("GbitsVersionLogic evidence contains client paths");
        }
    }
    // //// /验证完整性证据字段和摘要 ////

    // //// 验证重复应用补丁被拒绝 [@x380kkm 2026-08-12] ////
    private static void verifyRepeatedApplicationRejected(
            Path referencePath,
            Path patchedInputPath,
            Path outputPath,
            Path repeatedOutputPath,
            Path repeatedEvidencePath) throws Exception {
        try {
            GbitsVersionQueryProbePatch.main(new String[] {
                    referencePath.toString(),
                    patchedInputPath.toString(),
                    repeatedOutputPath.toString(),
                    repeatedEvidencePath.toString()
            });
        } catch (IllegalStateException error) {
            if (error.getMessage() != null
                    && (error.getMessage().contains(
                            "GbitsVersionLogic sdkDummy success sequence count must be one: 0")
                    || error.getMessage().contains(
                            "FFDec import changed GbitsVersionLogic before ABC patch"))) {
                return;
            }
            throw error;
        }
        throw new IllegalStateException("repeated GbitsVersionLogic patch was accepted");
    }
    // //// /验证重复应用补丁被拒绝 ////

    // //// 要求证据包含指定文本 [@x380kkm 2026-08-12] ////
    private static void requireContains(String evidence, String expected) {
        if (!evidence.contains(expected)) {
            throw new IllegalStateException("GbitsVersionLogic evidence is missing: " + expected);
        }
    }
    // //// /要求证据包含指定文本 ////
}
