// audience: internal
// # asset-extractor-preextracted-bundle-patch
// 此工具在预展开资源已存在的协议实验中跳过 AssetExtractor.start 对内置 bundle.zip 的读取.
// 运行前提是 Java 17, AbcMethodDigest 和 Starview 随附的 FFDec 库.
// 工具只将 readBinaryFileAsync 分支前的 getlocal 6 条件替换为 pushfalse.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushFalseIns;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Multiname;
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
import java.util.function.Predicate;

public final class AssetExtractorPreextractedBundlePatch {
    private static final String CLASS_NAME = "pinball.loading.initial.AssetExtractor";
    private static final String PATCH_METHOD_NAME = "start";
    private static final String BUNDLE_FILE_NAME = "bundle.zip";
    private static final String CALLBACK_METHOD_NAME = "completeReadPackFile";
    private static final String READ_METHOD_NAME = "readBinaryFileAsync";
    private static final int CONDITION_LOCAL_REGISTER = 6;

    // //// 阻止实例化 AssetExtractor 补丁器 [@x380kkm 2026-08-11] ////
    private AssetExtractorPreextractedBundlePatch() {
    }
    // //// /阻止实例化 AssetExtractor 补丁器 ////

    // //// 校验参数并执行预展开资源补丁 [@x380kkm 2026-08-11] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException(
                    "usage: AssetExtractorPreextractedBundlePatch "
                            + "<reference.swf> <input.swf> <output.swf> <evidence.json>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path inputPath = Path.of(args[1]).toAbsolutePath().normalize();
        Path outputPath = Path.of(args[2]).toAbsolutePath().normalize();
        Path evidencePath = Path.of(args[3]).toAbsolutePath().normalize();
        if (inputPath.equals(outputPath)) {
            throw new IllegalArgumentException("input and output SWF paths must differ");
        }

        Map<String, String> referenceDigests = digestAssetExtractorMethods(referencePath);
        SavedPatch savedPatch = patchAndSaveInput(inputPath, outputPath, referenceDigests);
        verifySavedPatch(outputPath, evidencePath, referenceDigests, savedPatch);
    }
    // //// /校验参数并执行预展开资源补丁 ////

    // //// 读取 SWF 并拒绝缺失文件 [@x380kkm 2026-08-11] ////
    static SWF loadSwf(Path path) throws Exception {
        if (!Files.isRegularFile(path)) {
            throw new IllegalArgumentException("SWF does not exist: " + path);
        }
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF 并拒绝缺失文件 ////

    // //// 读取指定 SWF 的全部 AssetExtractor 实例方法摘要 [@x380kkm 2026-08-11] ////
    static Map<String, String> digestAssetExtractorMethods(Path path) throws Exception {
        SWF swf = loadSwf(path);
        AssetExtractorClass assetExtractor = findAssetExtractorClass(swf);
        return AbcMethodDigest.digestInstanceMethods(
                assetExtractor.abc(), assetExtractor.classIndex());
    }
    // //// /读取指定 SWF 的全部 AssetExtractor 实例方法摘要 ////

    // //// 唯一定位 AssetExtractor 类 [@x380kkm 2026-08-11] ////
    static AssetExtractorClass findAssetExtractorClass(SWF swf) {
        List<AssetExtractorClass> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(CLASS_NAME);
            if (classIndex >= 0) {
                matches.add(new AssetExtractorClass(tag, abc, classIndex));
            }
        }
        if (matches.size() != 1) {
            throw new IllegalStateException("AssetExtractor class count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /唯一定位 AssetExtractor 类 ////

    // //// 唯一定位读取内置资源包的条件 [@x380kkm 2026-08-11] ////
    static int findBundleReadConditionIndex(ABC abc, MethodBody body) {
        List<Integer> matches = findBundleReadConditionIndices(
                abc,
                body,
                AssetExtractorPreextractedBundlePatch::isConditionLocalLoad);
        if (matches.size() != 1) {
            throw new IllegalStateException(
                    "AssetExtractor bundle read condition count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /唯一定位读取内置资源包的条件 ////

    // //// 将资源包读取条件替换为 false [@x380kkm 2026-08-11] ////
    private static void replaceBundleReadCondition(
            AssetExtractorClass assetExtractor,
            MethodBody body,
            int conditionIndex) {
        body.replaceInstruction(
                conditionIndex,
                new AVM2Instruction(0, new PushFalseIns(), new int[0]));
        body.markOffsets();
        body.setModified();
        ((Tag) assetExtractor.tag()).setModified(true);
    }
    // //// /将资源包读取条件替换为 false ////

    // //// 修改 input 并将 SWF 保存到临时路径 [@x380kkm 2026-08-11] ////
    private static SavedPatch patchAndSaveInput(
            Path inputPath,
            Path outputPath,
            Map<String, String> referenceDigests) throws Exception {
        SWF input = loadSwf(inputPath);
        AssetExtractorClass inputClass = findAssetExtractorClass(input);
        Map<String, String> inputDigests = AbcMethodDigest.digestInstanceMethods(
                inputClass.abc(), inputClass.classIndex());
        AbcMethodDigest.requireSameMethods(
                referenceDigests,
                inputDigests,
                "input AssetExtractor differs from reference");

        MethodBody startMethod = inputClass.method(PATCH_METHOD_NAME);
        int conditionIndex = findBundleReadConditionIndex(inputClass.abc(), startMethod);
        replaceBundleReadCondition(inputClass, startMethod, conditionIndex);
        Map<String, String> memoryDigests = AbcMethodDigest.digestInstanceMethods(
                inputClass.abc(), inputClass.classIndex());
        Set<String> memoryChanges = requireOnlyPatchMethodChanged(
                inputDigests,
                memoryDigests,
                "in-memory AssetExtractor patch");

        Files.createDirectories(outputPath.getParent());
        Path temporaryPath = outputPath.resolveSibling(outputPath.getFileName() + ".tmp");
        Files.deleteIfExists(temporaryPath);
        try (OutputStream output = Files.newOutputStream(temporaryPath)) {
            input.saveTo(output);
        }
        return new SavedPatch(temporaryPath, inputDigests, conditionIndex, memoryChanges);
    }
    // //// /修改 input 并将 SWF 保存到临时路径 ////

    // //// 重载临时 SWF 并写入补丁证据 [@x380kkm 2026-08-11] ////
    private static void verifySavedPatch(
            Path outputPath,
            Path evidencePath,
            Map<String, String> referenceDigests,
            SavedPatch savedPatch) throws Exception {
        SWF reloaded = loadSwf(savedPatch.temporaryPath());
        AssetExtractorClass outputClass = findAssetExtractorClass(reloaded);
        Map<String, String> outputDigests = AbcMethodDigest.digestInstanceMethods(
                outputClass.abc(), outputClass.classIndex());
        Set<String> outputChanges = requireOnlyPatchMethodChanged(
                savedPatch.inputDigests(),
                outputDigests,
                "saved AssetExtractor patch");
        if (!savedPatch.memoryChanges().equals(outputChanges)) {
            throw new IllegalStateException(
                    "saved AssetExtractor changed method set differs from memory: " + outputChanges);
        }
        requirePatchedBundleReadCondition(
                outputClass.abc(),
                outputClass.method(PATCH_METHOD_NAME),
                savedPatch.conditionIndex());

        moveAtomically(savedPatch.temporaryPath(), outputPath);
        writeEvidence(
                evidencePath,
                referenceDigests,
                savedPatch.inputDigests(),
                outputDigests,
                outputChanges,
                savedPatch.conditionIndex());
    }
    // //// /重载临时 SWF 并写入补丁证据 ////

    // //// 要求只有 start 方法摘要变化 [@x380kkm 2026-08-11] ////
    private static Set<String> requireOnlyPatchMethodChanged(
            Map<String, String> before,
            Map<String, String> after,
            String context) {
        Set<String> changedMethods = AbcMethodDigest.changedMethods(before, after);
        if (!changedMethods.equals(Set.of(PATCH_METHOD_NAME))) {
            throw new IllegalStateException(context + " changed unexpected methods: " + changedMethods);
        }
        return changedMethods;
    }
    // //// /要求只有 start 方法摘要变化 ////

    // //// 验证保存后的条件仍是 pushfalse [@x380kkm 2026-08-11] ////
    private static void requirePatchedBundleReadCondition(
            ABC abc,
            MethodBody body,
            int expectedConditionIndex) {
        List<Integer> originalConditions = findBundleReadConditionIndices(
                abc,
                body,
                AssetExtractorPreextractedBundlePatch::isConditionLocalLoad);
        if (!originalConditions.isEmpty()) {
            throw new IllegalStateException(
                    "saved AssetExtractor retains original bundle read condition: " + originalConditions);
        }

        List<Integer> patchedConditions = findBundleReadConditionIndices(
                abc,
                body,
                AssetExtractorPreextractedBundlePatch::isPushFalse);
        if (!patchedConditions.equals(List.of(expectedConditionIndex))) {
            throw new IllegalStateException(
                    "saved AssetExtractor patched condition differs: " + patchedConditions);
        }
    }
    // //// /验证保存后的条件仍是 pushfalse ////

    // //// 定位满足指定前置指令的资源包读取分支 [@x380kkm 2026-08-11] ////
    private static List<Integer> findBundleReadConditionIndices(
            ABC abc,
            MethodBody body,
            Predicate<AVM2Instruction> conditionMatcher) {
        List<AVM2Instruction> instructions = body.getCode().code;
        List<Integer> matches = new ArrayList<>();
        for (int index = 0; index + 1 < instructions.size(); index++) {
            AVM2Instruction condition = instructions.get(index);
            AVM2Instruction branch = instructions.get(index + 1);
            if (conditionMatcher.test(condition)
                    && branch.definition.instructionName.equals("iffalse")
                    && isBundleReadBranch(abc, instructions, index, branch)) {
                matches.add(index);
            }
        }
        return matches;
    }
    // //// /定位满足指定前置指令的资源包读取分支 ////

    // //// 验证条件分支调用内置资源包异步读取 [@x380kkm 2026-08-11] ////
    private static boolean isBundleReadBranch(
            ABC abc,
            List<AVM2Instruction> instructions,
            int conditionIndex,
            AVM2Instruction branch) {
        long targetAddress = branch.getTargetAddress();
        if (targetAddress <= branch.getAddress()) {
            return false;
        }

        boolean bundleFileSeen = false;
        boolean callbackSeen = false;
        for (int index = conditionIndex + 2; index < instructions.size(); index++) {
            AVM2Instruction instruction = instructions.get(index);
            if (instruction.getAddress() >= targetAddress) {
                break;
            }
            if (isPushedString(abc, instruction, BUNDLE_FILE_NAME)) {
                bundleFileSeen = true;
                continue;
            }
            if (bundleFileSeen && isCallbackReference(abc, instruction)) {
                callbackSeen = true;
                continue;
            }
            if (callbackSeen && isMethodCall(abc, instruction, READ_METHOD_NAME, 2)) {
                return true;
            }
        }
        return false;
    }
    // //// /验证条件分支调用内置资源包异步读取 ////

    // //// 判断指令读取局部变量 6 [@x380kkm 2026-08-11] ////
    private static boolean isConditionLocalLoad(AVM2Instruction instruction) {
        return instruction.definition.instructionName.equals("getlocal")
                && instruction.operands != null
                && instruction.operands.length == 1
                && instruction.operands[0] == CONDITION_LOCAL_REGISTER;
    }
    // //// /判断指令读取局部变量 6 ////

    // //// 判断指令压入 false [@x380kkm 2026-08-11] ////
    private static boolean isPushFalse(AVM2Instruction instruction) {
        return instruction.definition.instructionName.equals("pushfalse");
    }
    // //// /判断指令压入 false ////

    // //// 判断指令压入指定字符串 [@x380kkm 2026-08-11] ////
    private static boolean isPushedString(ABC abc, AVM2Instruction instruction, String expectedValue) {
        return instruction.definition.instructionName.equals("pushstring")
                && instruction.operands != null
                && instruction.operands.length == 1
                && expectedValue.equals(abc.constants.getString(instruction.operands[0]));
    }
    // //// /判断指令压入指定字符串 ////

    // //// 判断指令读取完成回调 [@x380kkm 2026-08-11] ////
    private static boolean isCallbackReference(ABC abc, AVM2Instruction instruction) {
        String instructionName = instruction.definition.instructionName;
        return (instructionName.equals("findproperty") || instructionName.equals("getproperty"))
                && referencesMultiname(abc, instruction, CALLBACK_METHOD_NAME);
    }
    // //// /判断指令读取完成回调 ////

    // //// 判断指令调用指定方法和参数数量 [@x380kkm 2026-08-11] ////
    private static boolean isMethodCall(
            ABC abc,
            AVM2Instruction instruction,
            String methodName,
            int argumentCount) {
        String instructionName = instruction.definition.instructionName;
        return (instructionName.equals("callproperty") || instructionName.equals("callpropvoid"))
                && instruction.operands != null
                && instruction.operands.length >= 2
                && instruction.operands[1] == argumentCount
                && referencesMultiname(abc, instruction, methodName);
    }
    // //// /判断指令调用指定方法和参数数量 ////

    // //// 判断指令引用指定 multiname [@x380kkm 2026-08-11] ////
    private static boolean referencesMultiname(
            ABC abc,
            AVM2Instruction instruction,
            String expectedName) {
        if (instruction.operands == null || instruction.operands.length == 0) {
            return false;
        }
        Multiname multiname = abc.constants.getMultiname(instruction.operands[0]);
        return multiname != null
                && multiname.hasOwnName()
                && expectedName.equals(abc.constants.getString(multiname.name_index));
    }
    // //// /判断指令引用指定 multiname ////

    // //// 原子替换输出文件 [@x380kkm 2026-08-11] ////
    private static void moveAtomically(Path source, Path target) throws Exception {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException error) {
            Files.move(source, target);
        }
    }
    // //// /原子替换输出文件 ////

    // //// 写入不含客户端数据的完整性证据 [@x380kkm 2026-08-11] ////
    private static void writeEvidence(
            Path evidencePath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            Map<String, String> outputDigests,
            Set<String> changedMethods,
            int conditionIndex) throws Exception {
        Files.createDirectories(evidencePath.getParent());
        String changed = changedMethods.stream()
                .map(AssetExtractorPreextractedBundlePatch::jsonString)
                .reduce((left, right) -> left + "," + right)
                .orElse("");
        String json = "{\n"
                + "  \"className\": " + jsonString(CLASS_NAME) + ",\n"
                + "  \"patchMethod\": " + jsonString(PATCH_METHOD_NAME) + ",\n"
                + "  \"digestVersion\": " + AbcMethodDigest.VERSION + ",\n"
                + "  \"methodOnly\": true,\n"
                + "  \"requiresPreextractedBundle\": true,\n"
                + "  \"changedMethods\": [" + changed + "],\n"
                + "  \"conditionIndex\": " + conditionIndex + ",\n"
                + "  \"start\": {\n"
                + "    \"referenceSha256\": " + jsonString(referenceDigests.get(PATCH_METHOD_NAME)) + ",\n"
                + "    \"inputSha256\": " + jsonString(inputDigests.get(PATCH_METHOD_NAME)) + ",\n"
                + "    \"outputSha256\": " + jsonString(outputDigests.get(PATCH_METHOD_NAME)) + "\n"
                + "  }\n"
                + "}\n";
        Files.writeString(evidencePath, json, StandardCharsets.UTF_8);
    }
    // //// /写入不含客户端数据的完整性证据 ////

    // //// 编码 JSON 字符串 [@x380kkm 2026-08-11] ////
    private static String jsonString(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }
    // //// /编码 JSON 字符串 ////

    // //// 保存 AssetExtractor 类定位结果 [@x380kkm 2026-08-11] ////
    record AssetExtractorClass(ABCContainerTag tag, ABC abc, int classIndex) {
        // //// 读取指定实例方法 [@x380kkm 2026-08-11] ////
        MethodBody method(String methodName) {
            return AbcMethodDigest.findInstanceMethod(abc, classIndex, methodName);
        }
        // //// /读取指定实例方法 ////
    }
    // //// /保存 AssetExtractor 类定位结果 ////

    // //// 保存待重载补丁的数据 [@x380kkm 2026-08-11] ////
    private record SavedPatch(
            Path temporaryPath,
            Map<String, String> inputDigests,
            int conditionIndex,
            Set<String> memoryChanges) {
    }
    // //// /保存待重载补丁的数据 ////
}
