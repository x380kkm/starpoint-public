// audience: internal
// # asset-extractor-preextracted-bundle-patch-test
// 此测试使用外部 CN SWF 验证 AssetExtractor 预展开资源补丁的唯一方法变化和证据.
// 运行前提是 Java 17, Starview 随附的 FFDec 库和未提交的 CN 客户端 SWF.

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Set;

public final class AssetExtractorPreextractedBundlePatchTest {
    private static final String PATCH_METHOD_NAME = "start";

    // //// 阻止实例化 AssetExtractor 补丁测试 [@x380kkm 2026-08-11] ////
    private AssetExtractorPreextractedBundlePatchTest() {
    }
    // //// /阻止实例化 AssetExtractor 补丁测试 ////

    // //// 运行真实 SWF 回归测试 [@x380kkm 2026-08-11] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: AssetExtractorPreextractedBundlePatchTest "
                            + "<reference.swf> <temporary-directory>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path temporaryDirectory = Path.of(args[1]).toAbsolutePath().normalize();
        Files.createDirectories(temporaryDirectory);
        Path outputPath = temporaryDirectory.resolve("asset-extractor-preextracted.swf");
        Path evidencePath = temporaryDirectory.resolve("asset-extractor-preextracted-evidence.json");
        Path repeatedOutputPath = temporaryDirectory.resolve("asset-extractor-preextracted-repeated.swf");
        Path repeatedEvidencePath = temporaryDirectory.resolve("asset-extractor-preextracted-repeated-evidence.json");
        deleteOutputs(outputPath, evidencePath, repeatedOutputPath, repeatedEvidencePath);

        ReferenceSnapshot reference = readReferenceSnapshot(referencePath);

        AssetExtractorPreextractedBundlePatch.main(new String[] {
                referencePath.toString(),
                referencePath.toString(),
                outputPath.toString(),
                evidencePath.toString()
        });

        Map<String, String> outputDigests =
                AssetExtractorPreextractedBundlePatch.digestAssetExtractorMethods(outputPath);
        verifyMethodChanges(reference.digests(), outputDigests);
        verifyEvidence(
                evidencePath,
                referencePath,
                outputPath,
                reference.digests(),
                outputDigests,
                reference.conditionIndex());
        verifyRepeatedApplicationRejected(
                referencePath,
                outputPath,
                repeatedOutputPath,
                repeatedEvidencePath);
        System.out.println("AssetExtractorPreextractedBundlePatchTest: PASS");
    }
    // //// /运行真实 SWF 回归测试 ////

    // //// 删除测试输出 [@x380kkm 2026-08-11] ////
    private static void deleteOutputs(Path... paths) throws Exception {
        for (Path path : paths) {
            Files.deleteIfExists(path);
            Files.deleteIfExists(path.resolveSibling(path.getFileName() + ".tmp"));
        }
    }
    // //// /删除测试输出 ////

    // //// 读取参考 SWF 摘要和原始条件索引 [@x380kkm 2026-08-11] ////
    private static ReferenceSnapshot readReferenceSnapshot(Path referencePath) throws Exception {
        AssetExtractorPreextractedBundlePatch.AssetExtractorClass referenceClass =
                AssetExtractorPreextractedBundlePatch.findAssetExtractorClass(
                        AssetExtractorPreextractedBundlePatch.loadSwf(referencePath));
        Map<String, String> digests = AbcMethodDigest.digestInstanceMethods(
                referenceClass.abc(), referenceClass.classIndex());
        int conditionIndex = AssetExtractorPreextractedBundlePatch.findBundleReadConditionIndex(
                referenceClass.abc(),
                referenceClass.method(PATCH_METHOD_NAME));
        return new ReferenceSnapshot(digests, conditionIndex);
    }
    // //// /读取参考 SWF 摘要和原始条件索引 ////

    // //// 验证只有 start 实例方法变化 [@x380kkm 2026-08-11] ////
    private static void verifyMethodChanges(
            Map<String, String> referenceDigests,
            Map<String, String> outputDigests) {
        Set<String> changedMethods = AbcMethodDigest.changedMethods(referenceDigests, outputDigests);
        if (!changedMethods.equals(Set.of(PATCH_METHOD_NAME))) {
            throw new IllegalStateException("unexpected AssetExtractor method changes: " + changedMethods);
        }
        for (String methodName : referenceDigests.keySet()) {
            boolean changed = !referenceDigests.get(methodName).equals(outputDigests.get(methodName));
            if (methodName.equals(PATCH_METHOD_NAME) != changed) {
                throw new IllegalStateException("AssetExtractor method change mismatch: " + methodName);
            }
        }
    }
    // //// /验证只有 start 实例方法变化 ////

    // //// 验证完整性证据字段和摘要 [@x380kkm 2026-08-11] ////
    private static void verifyEvidence(
            Path evidencePath,
            Path referencePath,
            Path outputPath,
            Map<String, String> referenceDigests,
            Map<String, String> outputDigests,
            int conditionIndex) throws Exception {
        String evidence = Files.readString(evidencePath, StandardCharsets.UTF_8);
        requireContains(evidence, "\"className\": \"pinball.loading.initial.AssetExtractor\"");
        requireContains(evidence, "\"patchMethod\": \"start\"");
        requireContains(evidence, "\"digestVersion\": " + AbcMethodDigest.VERSION);
        requireContains(evidence, "\"methodOnly\": true");
        requireContains(evidence, "\"requiresPreextractedBundle\": true");
        requireContains(evidence, "\"changedMethods\": [\"start\"]");
        requireContains(evidence, "\"conditionIndex\": " + conditionIndex);
        requireContains(
                evidence,
                "\"referenceSha256\": \"" + referenceDigests.get(PATCH_METHOD_NAME) + "\"");
        requireContains(
                evidence,
                "\"inputSha256\": \"" + referenceDigests.get(PATCH_METHOD_NAME) + "\"");
        requireContains(
                evidence,
                "\"outputSha256\": \"" + outputDigests.get(PATCH_METHOD_NAME) + "\"");
        if (evidence.contains(referencePath.toString()) || evidence.contains(outputPath.toString())) {
            throw new IllegalStateException("AssetExtractor evidence contains client paths");
        }
    }
    // //// /验证完整性证据字段和摘要 ////

    // //// 验证重复应用补丁被拒绝 [@x380kkm 2026-08-11] ////
    private static void verifyRepeatedApplicationRejected(
            Path referencePath,
            Path patchedInputPath,
            Path outputPath,
            Path evidencePath) throws Exception {
        try {
            AssetExtractorPreextractedBundlePatch.main(new String[] {
                    referencePath.toString(),
                    patchedInputPath.toString(),
                    outputPath.toString(),
                    evidencePath.toString()
            });
        } catch (IllegalStateException error) {
            if (error.getMessage() != null
                    && error.getMessage().contains("input AssetExtractor differs from reference")) {
                return;
            }
            throw error;
        }
        throw new IllegalStateException("repeated AssetExtractor patch was accepted");
    }
    // //// /验证重复应用补丁被拒绝 ////

    // //// 要求证据包含指定文本 [@x380kkm 2026-08-11] ////
    private static void requireContains(String evidence, String expected) {
        if (!evidence.contains(expected)) {
            throw new IllegalStateException("AssetExtractor evidence is missing: " + expected);
        }
    }
    // //// /要求证据包含指定文本 ////

    // //// 保存参考 SWF 摘要和条件索引 [@x380kkm 2026-08-11] ////
    private record ReferenceSnapshot(Map<String, String> digests, int conditionIndex) {
    }
    // //// /保存参考 SWF 摘要和条件索引 ////
}
