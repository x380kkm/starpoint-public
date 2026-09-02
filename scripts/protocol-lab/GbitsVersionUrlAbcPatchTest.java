// audience: internal
// # gbits-version-url-abc-patch-test
// 此测试验证版本地址补丁支持独立 reference/input 基线, 只修改两个已知方法, 并拒绝重复应用和不一致输入.

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Set;

public final class GbitsVersionUrlAbcPatchTest {
    private static final String PRIMARY_URL = "http://127.0.0.1:8001/shijtswy/version/";
    private static final String BACKUP_URL = "http://127.0.0.1:8001/shijtswy/version/";

    private GbitsVersionUrlAbcPatchTest() {
    }

    // //// 运行版本地址真实 SWF 回归测试 [@x380kkm 2026-08-13] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: GbitsVersionUrlAbcPatchTest <reference.swf> <temporary-directory>");
        }
        Path reference = Path.of(args[0]).toAbsolutePath().normalize();
        Path directory = Path.of(args[1]).toAbsolutePath().normalize();
        Files.createDirectories(directory);
        Path input = directory.resolve("input-copy.swf");
        Path output = directory.resolve("patched.swf");
        Path evidence = directory.resolve("evidence.json");
        Path repeated = directory.resolve("repeated.swf");
        Path repeatedEvidence = directory.resolve("repeated-evidence.json");
        Path mismatch = directory.resolve("mismatch.swf");
        Path mismatchEvidence = directory.resolve("mismatch-evidence.json");
        deleteOutputs(input, output, evidence, repeated, repeatedEvidence, mismatch, mismatchEvidence);
        Files.copy(reference, input);

        Map<String, String> referenceDigests = GbitsVersionUrlAbcPatch.digestVersionLogicMethods(reference);
        GbitsVersionUrlAbcPatch.main(new String[] {
                reference.toString(), input.toString(), output.toString(), evidence.toString(), PRIMARY_URL, BACKUP_URL
        });
        Map<String, String> outputDigests = GbitsVersionUrlAbcPatch.digestVersionLogicMethods(output);
        Set<String> changed = AbcMethodDigest.changedMethods(referenceDigests, outputDigests);
        if (!changed.equals(Set.of(
                GbitsVersionUrlAbcPatch.PRIMARY_METHOD_NAME,
                GbitsVersionUrlAbcPatch.BACKUP_METHOD_NAME))) {
            throw new IllegalStateException("unexpected changed methods: " + changed);
        }
        String evidenceText = Files.readString(evidence, StandardCharsets.UTF_8);
        requireContains(evidenceText, "\"methodOnly\": true");
        requireContains(evidenceText, "\"targetUrl\": \"" + PRIMARY_URL + "\"");
        requireContains(evidenceText, "\"targetUrl\": \"" + BACKUP_URL + "\"");
        if (evidenceText.contains(reference.toString()) || evidenceText.contains(input.toString())) {
            throw new IllegalStateException("URL patch evidence contains client paths");
        }

        verifyRejected(() -> GbitsVersionUrlAbcPatch.main(new String[] {
                reference.toString(), output.toString(), repeated.toString(), repeatedEvidence.toString(), PRIMARY_URL, BACKUP_URL
        }), "repeated URL patch", "input differs from reference");

        GbitsVersionUrlAbcPatch.main(new String[] {
                reference.toString(), input.toString(), mismatch.toString(), mismatchEvidence.toString(),
                "http://127.0.0.1:8002/shijtswy/version/", "http://127.0.0.1:8002/shijtswy/version/"
        });
        verifyRejected(() -> GbitsVersionUrlAbcPatch.main(new String[] {
                reference.toString(), mismatch.toString(), repeated.toString(), repeatedEvidence.toString(), PRIMARY_URL, BACKUP_URL
        }), "mismatched reference/input baseline", "input differs from reference");
        System.out.println("GbitsVersionUrlAbcPatchTest: PASS");
    }
    // //// /运行版本地址真实 SWF 回归测试 ////

    private static void deleteOutputs(Path... paths) throws Exception {
        for (Path path : paths) {
            Files.deleteIfExists(path);
            Files.deleteIfExists(path.resolveSibling(path.getFileName() + ".tmp"));
        }
    }

    private static void verifyRejected(ThrowingAction action, String name, String expectedMessagePart) throws Exception {
        try {
            action.run();
        } catch (IllegalStateException expected) {
            if (expected.getMessage() != null && expected.getMessage().contains(expectedMessagePart)) {
                return;
            }
            throw new IllegalStateException(name + " failed with an unexpected rejection: " + expected.getMessage());
        }
        throw new IllegalStateException(name + " was accepted");
    }

    private static void requireContains(String content, String expected) {
        if (!content.contains(expected)) {
            throw new IllegalStateException("evidence is missing: " + expected);
        }
    }

    @FunctionalInterface
    private interface ThrowingAction {
        void run() throws Exception;
    }
}
