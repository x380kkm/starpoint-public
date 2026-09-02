// audience: internal
// # gbits-version-query-probe-patch
// 此工具只移除 GbitsVersionLogic.isQuerySuccess 对 DevConfig.sdkDummy 的提前成功判断.
// 运行前提是 Java 17, AbcMethodDigest 和 Starview 随附的 FFDec 库.
// 工具保持 publishTarget 和 versions 判断不变, 并拒绝修改其他实例方法.

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

public final class GbitsVersionQueryProbePatch {
    private static final String CLASS_NAME = "pinball.gbits.logic.GbitsVersionLogic";
    private static final String PATCH_METHOD_NAME = "isQuerySuccess";
    private static final String DEV_CONFIG_NAMESPACE = "pinball.config.core";
    private static final String DEV_CONFIG_CLASS_NAME = "DevConfig";
    private static final String SDK_DUMMY_PROPERTY_NAME = "sdkDummy";
    private static final String PUBLISH_TARGET_PROPERTY_NAME = "publishTarget";
    private static final String VERSIONS_PROPERTY_NAME = "versions";

    // //// 阻止实例化版本查询补丁器 [@x380kkm 2026-08-12] ////
    private GbitsVersionQueryProbePatch() {
    }
    // //// /阻止实例化版本查询补丁器 ////

    // //// 校验参数并执行方法级补丁 [@x380kkm 2026-08-12] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException(
                    "usage: GbitsVersionQueryProbePatch "
                            + "<reference.swf> <input.swf> <output.swf> <evidence.json>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path inputPath = Path.of(args[1]).toAbsolutePath().normalize();
        Path outputPath = Path.of(args[2]).toAbsolutePath().normalize();
        Path evidencePath = Path.of(args[3]).toAbsolutePath().normalize();
        if (inputPath.equals(outputPath)) {
            throw new IllegalArgumentException("input and output SWF paths must differ");
        }

        if (referencePath.equals(outputPath)) {
            throw new IllegalArgumentException("reference and output SWF paths must differ");
        }
        Map<String, String> referenceDigests = digestVersionLogicMethods(referencePath);
        Map<String, String> inputDigests = digestVersionLogicMethods(inputPath);
        AbcMethodDigest.requireSameMethods(
                referenceDigests,
                inputDigests,
                "FFDec import changed GbitsVersionLogic before ABC patch");
        SavedPatch savedPatch = patchAndSaveInput(inputPath, outputPath);
        verifySavedPatch(outputPath, evidencePath, referenceDigests, savedPatch);
    }
    // //// /校验参数并执行方法级补丁 ////

    // //// 读取 SWF 并拒绝缺失文件 [@x380kkm 2026-08-12] ////
    static SWF loadSwf(Path path) throws Exception {
        if (!Files.isRegularFile(path)) {
            throw new IllegalArgumentException("SWF does not exist: " + path);
        }
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF 并拒绝缺失文件 ////

    // //// 读取版本逻辑的全部实例方法摘要 [@x380kkm 2026-08-12] ////
    static Map<String, String> digestVersionLogicMethods(Path path) throws Exception {
        VersionLogicClass versionLogic = findVersionLogicClass(loadSwf(path));
        return AbcMethodDigest.digestInstanceMethods(versionLogic.abc(), versionLogic.classIndex());
    }
    // //// /读取版本逻辑的全部实例方法摘要 ////

    // //// 唯一定位版本逻辑类 [@x380kkm 2026-08-12] ////
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

    // //// 唯一定位 sdkDummy 提前成功指令范围 [@x380kkm 2026-08-12] ////
    static InstructionRange findSdkDummySuccessRanges(ABC abc, MethodBody body) {
        List<InstructionRange> matches = findSdkDummySuccessRangeCandidates(abc, body);
        if (matches.size() != 1) {
            throw new IllegalStateException(
                    "GbitsVersionLogic sdkDummy success sequence count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /唯一定位 sdkDummy 提前成功指令范围 ////

    // //// 修改输入 SWF 并保存临时结果 [@x380kkm 2026-08-12] ////
    private static SavedPatch patchAndSaveInput(
            Path inputPath,
            Path outputPath) throws Exception {
        SWF input = loadSwf(inputPath);
        VersionLogicClass inputClass = findVersionLogicClass(input);
        Map<String, String> inputDigests = AbcMethodDigest.digestInstanceMethods(
                inputClass.abc(), inputClass.classIndex());

        MethodBody method = inputClass.method(PATCH_METHOD_NAME);
        requireRemainingChecks(inputClass.abc(), method);
        InstructionRange range = findSdkDummySuccessRanges(inputClass.abc(), method);
        patchSdkDummySuccess(inputClass, method, range);
        requirePatchedSdkDummyCondition(inputClass.abc(), method, range);
        requireRemainingChecks(inputClass.abc(), method);

        Map<String, String> memoryDigests = AbcMethodDigest.digestInstanceMethods(
                inputClass.abc(), inputClass.classIndex());
        Set<String> memoryChanges = requireOnlyPatchMethodChanged(
                inputDigests,
                memoryDigests,
                "in-memory GbitsVersionLogic patch");

        Files.createDirectories(outputPath.getParent());
        Path temporaryPath = outputPath.resolveSibling(outputPath.getFileName() + ".tmp");
        Files.deleteIfExists(temporaryPath);
        try (OutputStream output = Files.newOutputStream(temporaryPath)) {
            input.saveTo(output);
        }
        return new SavedPatch(temporaryPath, inputDigests, range, memoryChanges);
    }
    // //// /修改输入 SWF 并保存临时结果 ////

    // //// 重载临时 SWF 并写入完整性证据 [@x380kkm 2026-08-12] ////
    private static void verifySavedPatch(
            Path outputPath,
            Path evidencePath,
            Map<String, String> referenceDigests,
            SavedPatch savedPatch) throws Exception {
        SWF reloaded = loadSwf(savedPatch.temporaryPath());
        VersionLogicClass outputClass = findVersionLogicClass(reloaded);
        Map<String, String> outputDigests = AbcMethodDigest.digestInstanceMethods(
                outputClass.abc(), outputClass.classIndex());
        Set<String> outputChanges = requireOnlyPatchMethodChanged(
                savedPatch.inputDigests(),
                outputDigests,
                "saved GbitsVersionLogic patch");
        if (!savedPatch.memoryChanges().equals(outputChanges)) {
            throw new IllegalStateException(
                    "saved GbitsVersionLogic changed method set differs from memory: " + outputChanges);
        }

        MethodBody outputMethod = outputClass.method(PATCH_METHOD_NAME);
        requirePatchedSdkDummyCondition(outputClass.abc(), outputMethod, savedPatch.range());
        requireRemainingChecks(outputClass.abc(), outputMethod);

        moveAtomically(savedPatch.temporaryPath(), outputPath);
        writeEvidence(
                evidencePath,
                referenceDigests,
                savedPatch.inputDigests(),
                outputDigests,
                outputChanges,
                savedPatch.range());
    }
    // //// /重载临时 SWF 并写入完整性证据 ////

    // //// 将 sdkDummy 条件替换为 false 并保持栈平衡 [@x380kkm 2026-08-12] ////
    private static void patchSdkDummySuccess(
            VersionLogicClass versionLogic,
            MethodBody body,
            InstructionRange range) {
        body.replaceInstruction(
                range.sdkDummyClassIndex(),
                new AVM2Instruction(0, new PushFalseIns(), new int[0]));
        body.removeInstruction(range.sdkDummyPropertyIndex());
        body.markOffsets();
        body.setModified();
        ((Tag) versionLogic.tag()).setModified(true);
    }
    // //// /将 sdkDummy 条件替换为 false 并保持栈平衡 ////

    // //// 查找原始 sdkDummy 提前成功序列 [@x380kkm 2026-08-12] ////
    private static List<InstructionRange> findSdkDummySuccessRangeCandidates(ABC abc, MethodBody body) {
        List<AVM2Instruction> instructions = body.getCode().code;
        List<InstructionRange> matches = new ArrayList<>();
        for (int index = 0; index + 2 < instructions.size(); index++) {
            AVM2Instruction classReference = instructions.get(index);
            AVM2Instruction propertyRead = instructions.get(index + 1);
            if (!isGetLex(
                    abc,
                    classReference,
                    DEV_CONFIG_NAMESPACE,
                    DEV_CONFIG_CLASS_NAME)
                    || !isNamedInstruction(
                            abc,
                            propertyRead,
                            "getproperty",
                            SDK_DUMMY_PROPERTY_NAME)) {
                continue;
            }

            int branchIndex = index + 2;
            if (instructions.get(branchIndex).definition.instructionName.equals("dup")) {
                branchIndex++;
            }
            if (branchIndex < instructions.size()
                    && instructions.get(branchIndex).definition.instructionName.equals("iftrue")) {
                matches.add(new InstructionRange(index, index + 1, branchIndex));
            }
        }
        return matches;
    }
    // //// /查找原始 sdkDummy 提前成功序列 ////

    // //// 验证保存后的 sdkDummy 条件保持 false 和 pop [@x380kkm 2026-08-12] ////
    private static void requirePatchedSdkDummyCondition(
            ABC abc,
            MethodBody body,
            InstructionRange range) {
        if (!findSdkDummySuccessRangeCandidates(abc, body).isEmpty()) {
            throw new IllegalStateException("saved GbitsVersionLogic retains sdkDummy success sequence");
        }
        List<AVM2Instruction> instructions = body.getCode().code;
        int savedBranchIndex = range.branchIndex() - 1;
        if (savedBranchIndex >= instructions.size()
                || !instructions.get(range.sdkDummyClassIndex()).definition.instructionName.equals("pushfalse")
                || !instructions.get(savedBranchIndex).definition.instructionName.equals("iftrue")) {
            throw new IllegalStateException("saved GbitsVersionLogic patched instruction sequence differs");
        }
    }
    // //// /验证保存后的 sdkDummy 条件保持 false 和 pop ////

    // //// 验证发布目标和版本数据判断仍存在 [@x380kkm 2026-08-12] ////
    private static void requireRemainingChecks(ABC abc, MethodBody body) {
        if (countNamedInstructions(abc, body, PUBLISH_TARGET_PROPERTY_NAME) != 1) {
            throw new IllegalStateException("GbitsVersionLogic publishTarget check count must be one");
        }
        int versionsCheckCount = countNamedInstructions(abc, body, VERSIONS_PROPERTY_NAME);
        if (versionsCheckCount < 1 || versionsCheckCount > 2) {
            throw new IllegalStateException(
                    "GbitsVersionLogic versions check count must be one or two: "
                            + versionsCheckCount);
        }
    }
    // //// /验证发布目标和版本数据判断仍存在 ////

    // //// 统计方法内指定 multiname 的引用 [@x380kkm 2026-08-12] ////
    private static int countNamedInstructions(ABC abc, MethodBody body, String name) {
        int count = 0;
        for (AVM2Instruction instruction : body.getCode().code) {
            if (referencesMultiname(abc, instruction, null, name)) {
                count++;
            }
        }
        return count;
    }
    // //// /统计方法内指定 multiname 的引用 ////

    // //// 判断 getlex 指令引用指定类 [@x380kkm 2026-08-12] ////
    private static boolean isGetLex(
            ABC abc,
            AVM2Instruction instruction,
            String namespace,
            String name) {
        return instruction.definition.instructionName.equals("getlex")
                && referencesMultiname(abc, instruction, namespace, name);
    }
    // //// /判断 getlex 指令引用指定类 ////

    // //// 判断指定指令引用指定属性 [@x380kkm 2026-08-12] ////
    private static boolean isNamedInstruction(
            ABC abc,
            AVM2Instruction instruction,
            String instructionName,
            String name) {
        return instruction.definition.instructionName.equals(instructionName)
                && referencesMultiname(abc, instruction, null, name);
    }
    // //// /判断指定指令引用指定属性 ////

    // //// 判断指令引用指定 multiname [@x380kkm 2026-08-12] ////
    private static boolean referencesMultiname(
            ABC abc,
            AVM2Instruction instruction,
            String expectedNamespace,
            String expectedName) {
        if (instruction.operands == null || instruction.operands.length == 0) {
            return false;
        }
        Multiname multiname = abc.constants.getMultiname(instruction.operands[0]);
        if (multiname == null
                || !multiname.hasOwnName()
                || !expectedName.equals(abc.constants.getString(multiname.name_index))) {
            return false;
        }
        if (expectedNamespace == null) {
            return true;
        }
        return multiname.getNamespace(abc.constants).hasName(expectedNamespace, abc.constants);
    }
    // //// /判断指令引用指定 multiname ////

    // //// 要求只有 isQuerySuccess 方法摘要变化 [@x380kkm 2026-08-12] ////
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
    // //// /要求只有 isQuerySuccess 方法摘要变化 ////

    // //// 原子替换输出文件 [@x380kkm 2026-08-12] ////
    private static void moveAtomically(Path source, Path target) throws Exception {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException error) {
            Files.move(source, target);
        }
    }
    // //// /原子替换输出文件 ////

    // //// 写入不含客户端数据的完整性证据 [@x380kkm 2026-08-12] ////
    private static void writeEvidence(
            Path evidencePath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            Map<String, String> outputDigests,
            Set<String> changedMethods,
            InstructionRange range) throws Exception {
        Files.createDirectories(evidencePath.getParent());
        String changed = changedMethods.stream()
                .map(GbitsVersionQueryProbePatch::jsonString)
                .reduce((left, right) -> left + "," + right)
                .orElse("");
        String json = "{\n"
                + "  \"className\": " + jsonString(CLASS_NAME) + ",\n"
                + "  \"patchMethod\": " + jsonString(PATCH_METHOD_NAME) + ",\n"
                + "  \"digestVersion\": " + AbcMethodDigest.VERSION + ",\n"
                + "  \"methodOnly\": true,\n"
                + "  \"removesSdkDummyEarlySuccess\": true,\n"
                + "  \"preservesPublishTargetCheck\": true,\n"
                + "  \"preservesVersionsCheck\": true,\n"
                + "  \"changedMethods\": [" + changed + "],\n"
                + "  \"instructionRange\": {\n"
                + "    \"sdkDummyClassIndex\": " + range.sdkDummyClassIndex() + ",\n"
                + "    \"sdkDummyPropertyIndex\": " + range.sdkDummyPropertyIndex() + ",\n"
                + "    \"branchIndex\": " + range.branchIndex() + "\n"
                + "  },\n"
                + "  \"isQuerySuccess\": {\n"
                + "    \"referenceSha256\": " + jsonString(referenceDigests.get(PATCH_METHOD_NAME)) + ",\n"
                + "    \"inputSha256\": " + jsonString(inputDigests.get(PATCH_METHOD_NAME)) + ",\n"
                + "    \"outputSha256\": " + jsonString(outputDigests.get(PATCH_METHOD_NAME)) + "\n"
                + "  }\n"
                + "}\n";
        Files.writeString(evidencePath, json, StandardCharsets.UTF_8);
    }
    // //// /写入不含客户端数据的完整性证据 ////

    // //// 编码 JSON 字符串 [@x380kkm 2026-08-12] ////
    private static String jsonString(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }
    // //// /编码 JSON 字符串 ////

    record VersionLogicClass(ABCContainerTag tag, ABC abc, int classIndex) {
        // //// 读取指定实例方法 [@x380kkm 2026-08-12] ////
        MethodBody method(String methodName) {
            return AbcMethodDigest.findInstanceMethod(abc, classIndex, methodName);
        }
        // //// /读取指定实例方法 ////
    }

    record InstructionRange(int sdkDummyClassIndex, int sdkDummyPropertyIndex, int branchIndex) {
    }

    private record SavedPatch(
            Path temporaryPath,
            Map<String, String> inputDigests,
            InstructionRange range,
            Set<String> memoryChanges) {
    }
}
