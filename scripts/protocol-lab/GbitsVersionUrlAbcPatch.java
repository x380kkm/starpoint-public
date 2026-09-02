// audience: internal
// # gbits-version-url-abc-patch
// 此工具直接修改 GbitsVersionLogic.queryVersion 和 onQueryErrorDefault 的版本地址字符串.
// 工具从同一版本的 reference 和 input SWF 建立方法摘要基线, 不依赖 FFDec 重编译该类.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushStringIns;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import com.jpexs.decompiler.flash.tags.Tag;

import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

public final class GbitsVersionUrlAbcPatch {
    static final String CLASS_NAME = "pinball.gbits.logic.GbitsVersionLogic";
    static final String PRIMARY_METHOD_NAME = "queryVersion";
    static final String BACKUP_METHOD_NAME = "onQueryErrorDefault";
    static final String PRIMARY_SOURCE_URL = "https://update.leiting.com/shijtswy/version/";
    static final String BACKUP_SOURCE_URL = "https://update.roguelike.com/shijtswy/version/";

    private GbitsVersionUrlAbcPatch() {
    }

    // //// 校验参数并应用版本地址补丁 [@x380kkm 2026-08-13] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 6) {
            throw new IllegalArgumentException(
                    "usage: GbitsVersionUrlAbcPatch <reference.swf> <input.swf> "
                            + "<output.swf> <evidence.json> <primary-url> <backup-url>");
        }

        Path referencePath = normalize(args[0]);
        Path inputPath = normalize(args[1]);
        Path outputPath = normalize(args[2]);
        Path evidencePath = normalize(args[3]);
        String primaryUrl = args[4];
        String backupUrl = args[5];
        if (inputPath.equals(outputPath) || referencePath.equals(outputPath)) {
            throw new IllegalArgumentException("reference, input and output SWF paths must differ");
        }
        requireUrl(primaryUrl, "primary-url");
        requireUrl(backupUrl, "backup-url");

        Map<String, String> referenceDigests = digestVersionLogicMethods(referencePath);
        Map<String, String> inputDigests = digestVersionLogicMethods(inputPath);
        requireSameExcept(
                referenceDigests,
                inputDigests,
                Set.of("isQuerySuccess"),
                "GbitsVersionLogic input differs from reference before URL patch");

        SavedPatch savedPatch = patchAndSaveInput(inputPath, outputPath, primaryUrl, backupUrl);
        verifySavedPatch(
                outputPath,
                evidencePath,
                referenceDigests,
                primaryUrl,
                backupUrl,
                savedPatch);
    }
    // //// /校验参数并应用版本地址补丁 ////

    // //// 读取 SWF 并拒绝缺失文件 [@x380kkm 2026-08-13] ////
    static SWF loadSwf(Path path) throws Exception {
        if (!Files.isRegularFile(path)) {
            throw new IllegalArgumentException("SWF does not exist: " + path);
        }
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF 并拒绝缺失文件 ////

    // //// 读取版本逻辑全部实例方法摘要 [@x380kkm 2026-08-13] ////
    static Map<String, String> digestVersionLogicMethods(Path path) throws Exception {
        VersionLogicClass versionLogic = findVersionLogicClass(loadSwf(path));
        return AbcMethodDigest.digestInstanceMethods(versionLogic.abc(), versionLogic.classIndex());
    }
    // //// /读取版本逻辑全部实例方法摘要 ////

    // //// 唯一定位版本逻辑类 [@x380kkm 2026-08-13] ////
    static VersionLogicClass findVersionLogicClass(SWF swf) {
        List<VersionLogicClass> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(CLASS_NAME);
            if (classIndex >= 0) {
                matches.add(new VersionLogicClass(tag, abc, classIndex));
            }
        }
        if (matches.size() != 1) {
            throw new IllegalStateException("GbitsVersionLogic class count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /唯一定位版本逻辑类 ////

    // //// 修改 input 的两个版本地址并保存临时文件 [@x380kkm 2026-08-13] ////
    private static SavedPatch patchAndSaveInput(
            Path inputPath,
            Path outputPath,
            String primaryUrl,
            String backupUrl) throws Exception {
        SWF input = loadSwf(inputPath);
        VersionLogicClass versionLogic = findVersionLogicClass(input);
        Map<String, String> inputDigests = AbcMethodDigest.digestInstanceMethods(
                versionLogic.abc(), versionLogic.classIndex());
        patchUrl(
                versionLogic,
                versionLogic.method(PRIMARY_METHOD_NAME),
                PRIMARY_SOURCE_URL,
                primaryUrl);
        patchUrl(
                versionLogic,
                versionLogic.method(BACKUP_METHOD_NAME),
                BACKUP_SOURCE_URL,
                backupUrl);
        Map<String, String> memoryDigests = AbcMethodDigest.digestInstanceMethods(
                versionLogic.abc(), versionLogic.classIndex());
        Set<String> memoryChanges = requireOnlyUrlMethods(inputDigests, memoryDigests, "in-memory URL patch");

        Files.createDirectories(outputPath.toAbsolutePath().getParent());
        Path temporaryPath = outputPath.resolveSibling(outputPath.getFileName() + ".tmp");
        Files.deleteIfExists(temporaryPath);
        try (OutputStream output = Files.newOutputStream(temporaryPath)) {
            input.saveTo(output);
        }
        return new SavedPatch(temporaryPath, inputDigests, memoryChanges);
    }
    // //// /修改 input 的两个版本地址并保存临时文件 ////

    // //// 重载并验证版本地址补丁 [@x380kkm 2026-08-13] ////
    private static void verifySavedPatch(
            Path outputPath,
            Path evidencePath,
            Map<String, String> referenceDigests,
            String primaryUrl,
            String backupUrl,
            SavedPatch savedPatch) throws Exception {
        SWF reloaded = loadSwf(savedPatch.temporaryPath());
        VersionLogicClass outputClass = findVersionLogicClass(reloaded);
        Map<String, String> outputDigests = AbcMethodDigest.digestInstanceMethods(
                outputClass.abc(), outputClass.classIndex());
        Set<String> outputChanges = requireOnlyUrlMethods(
                savedPatch.inputDigests(),
                outputDigests,
                "saved GbitsVersionLogic URL patch");
        if (!savedPatch.memoryChanges().equals(outputChanges)) {
            throw new IllegalStateException("saved URL patch changed method set differs from memory: " + outputChanges);
        }
        requirePatchedUrl(outputClass.method(PRIMARY_METHOD_NAME), outputClass.abc(), primaryUrl, PRIMARY_SOURCE_URL);
        requirePatchedUrl(outputClass.method(BACKUP_METHOD_NAME), outputClass.abc(), backupUrl, BACKUP_SOURCE_URL);

        moveAtomically(savedPatch.temporaryPath(), outputPath);
        writeEvidence(evidencePath, referenceDigests, savedPatch.inputDigests(), outputDigests, primaryUrl, backupUrl);
    }
    // //// /重载并验证版本地址补丁 ////

    // //// 精确替换方法中的一个版本地址 [@x380kkm 2026-08-13] ////
    private static void patchUrl(
            VersionLogicClass versionLogic,
            MethodBody body,
            String sourceUrl,
            String targetUrl) {
        List<AVM2Instruction> matches = new ArrayList<>();
        for (AVM2Instruction instruction : body.getCode().code) {
            if (isPushString(versionLogic.abc(), instruction, sourceUrl)) {
                matches.add(instruction);
            }
        }
        if (matches.size() != 1) {
            throw new IllegalStateException("version URL occurrence must be one: method="
                    + sourceUrl + " count=" + matches.size());
        }
        AVM2Instruction match = matches.get(0);
        int index = body.getCode().code.indexOf(match);
        int targetStringIndex = versionLogic.abc().constants.addString(targetUrl);
        body.replaceInstruction(index, new AVM2Instruction(0, new PushStringIns(), new int[] {targetStringIndex}));
        body.markOffsets();
        body.setModified();
        ((Tag) versionLogic.tag()).setModified(true);
    }
    // //// /精确替换方法中的一个版本地址 ////

    // //// 验证方法只保留目标版本地址 [@x380kkm 2026-08-13] ////
    private static void requirePatchedUrl(MethodBody body, ABC abc, String targetUrl, String sourceUrl) {
        int targetCount = 0;
        int sourceCount = 0;
        for (AVM2Instruction instruction : body.getCode().code) {
            if (isPushString(abc, instruction, targetUrl)) {
                targetCount++;
            }
            if (isPushString(abc, instruction, sourceUrl)) {
                sourceCount++;
            }
        }
        if (targetCount != 1 || sourceCount != 0) {
            throw new IllegalStateException("patched version URL shape is invalid: target="
                    + targetCount + " source=" + sourceCount);
        }
    }
    // //// /验证方法只保留目标版本地址 ////

    // //// 判断指令是否压入指定字符串 [@x380kkm 2026-08-13] ////
    private static boolean isPushString(ABC abc, AVM2Instruction instruction, String value) {
        return instruction.definition.instructionName.equals("pushstring")
                && instruction.operands != null
                && instruction.operands.length == 1
                && value.equals(abc.constants.getString(instruction.operands[0]));
    }
    // //// /判断指令是否压入指定字符串 ////

    // //// 要求只有两个版本方法摘要变化 [@x380kkm 2026-08-13] ////
    private static Set<String> requireOnlyUrlMethods(
            Map<String, String> before,
            Map<String, String> after,
            String context) {
        Set<String> changedMethods = AbcMethodDigest.changedMethods(before, after);
        Set<String> expected = Set.of(PRIMARY_METHOD_NAME, BACKUP_METHOD_NAME);
        if (!changedMethods.equals(expected)) {
            throw new IllegalStateException(context + " changed unexpected methods: " + changedMethods);
        }
        return changedMethods;
    }
    // //// /要求只有两个版本方法摘要变化 ////

    // //// 允许输入只带有先前的成功判断补丁 [@x380kkm 2026-08-13] ////
    private static void requireSameExcept(
            Map<String, String> reference,
            Map<String, String> input,
            Set<String> allowedChanges,
            String context) {
        Set<String> changed = new TreeSet<>(AbcMethodDigest.changedMethods(reference, input));
        changed.removeAll(allowedChanges);
        if (!changed.isEmpty()) {
            throw new IllegalStateException(context + ": " + changed);
        }
    }
    // //// /允许输入只带有先前的成功判断补丁 ////

    // //// 写入不含客户端路径的补丁证据 [@x380kkm 2026-08-13] ////
    private static void writeEvidence(
            Path evidencePath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            Map<String, String> outputDigests,
            String primaryUrl,
            String backupUrl) throws Exception {
        Files.createDirectories(evidencePath.toAbsolutePath().getParent());
        String json = "{\n"
                + "  \"className\": \"" + CLASS_NAME + "\",\n"
                + "  \"digestVersion\": " + AbcMethodDigest.VERSION + ",\n"
                + "  \"methodOnly\": true,\n"
                + "  \"changedMethods\": [\"" + PRIMARY_METHOD_NAME + "\",\"" + BACKUP_METHOD_NAME + "\"],\n"
                + "  \"inputChangesFromReference\": " + jsonArray(AbcMethodDigest.changedMethods(referenceDigests, inputDigests)) + ",\n"
                + "  \"queryVersion\": {\n"
                + "    \"sourceUrl\": \"" + PRIMARY_SOURCE_URL + "\",\n"
                + "    \"targetUrl\": \"" + jsonEscape(primaryUrl) + "\",\n"
                + "    \"referenceSha256\": \"" + referenceDigests.get(PRIMARY_METHOD_NAME) + "\",\n"
                + "    \"inputSha256\": \"" + inputDigests.get(PRIMARY_METHOD_NAME) + "\",\n"
                + "    \"outputSha256\": \"" + outputDigests.get(PRIMARY_METHOD_NAME) + "\"\n"
                + "  },\n"
                + "  \"onQueryErrorDefault\": {\n"
                + "    \"sourceUrl\": \"" + BACKUP_SOURCE_URL + "\",\n"
                + "    \"targetUrl\": \"" + jsonEscape(backupUrl) + "\",\n"
                + "    \"referenceSha256\": \"" + referenceDigests.get(BACKUP_METHOD_NAME) + "\",\n"
                + "    \"inputSha256\": \"" + inputDigests.get(BACKUP_METHOD_NAME) + "\",\n"
                + "    \"outputSha256\": \"" + outputDigests.get(BACKUP_METHOD_NAME) + "\"\n"
                + "  }\n"
                + "}\n";
        Files.writeString(evidencePath, json, StandardCharsets.UTF_8);
    }
    // //// /写入不含客户端路径的补丁证据 ////

    private static Path normalize(String value) {
        return Path.of(value).toAbsolutePath().normalize();
    }

    private static void requireUrl(String value, String name) {
        if (value == null || value.isBlank() || value.indexOf('\"') >= 0 || value.indexOf('\n') >= 0) {
            throw new IllegalArgumentException(name + " must be a non-empty single-line URL");
        }
    }

    private static String jsonEscape(String value) {
        return value.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static String jsonArray(Set<String> values) {
        return values.stream()
                .sorted()
                .map(value -> "\"" + jsonEscape(value) + "\"")
                .reduce((left, right) -> left + "," + right)
                .map(value -> "[" + value + "]")
                .orElse("[]");
    }

    private static void moveAtomically(Path source, Path target) throws Exception {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException error) {
            Files.move(source, target);
        }
    }

    record VersionLogicClass(ABCContainerTag tag, ABC abc, int classIndex) {
        MethodBody method(String methodName) {
            return AbcMethodDigest.findInstanceMethod(abc, classIndex, methodName);
        }
    }

    private record SavedPatch(
            Path temporaryPath,
            Map<String, String> inputDigests,
            Set<String> memoryChanges) {
    }
}
