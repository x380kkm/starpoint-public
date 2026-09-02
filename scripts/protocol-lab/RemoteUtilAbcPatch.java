// audience: internal
// # remote-util-abc-patch
// 此工具只跳过探测 APK 中 RemoteUtil.getURLRequest 的雷霆原生设备信息调用.
// 运行前提是 Java 17 和 Starview 随附的 FFDec 库.
// 工具拒绝修改 requestCompleteHandler 或任何未声明方法的签名, 方法体和引用常量语义.

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

public final class RemoteUtilAbcPatch {
    private static final String CLASS_NAME = "pinball.context.remote.RemoteUtil";
    private static final String REQUEST_METHOD_NAME = "getURLRequest";
    private static final String RESPONSE_METHOD_NAME = "requestCompleteHandler";
    private static final String EXTENSION_CLASS_NAME = "LeitingSDKExtension";
    private static final String EXTENSION_NAMESPACE = "com.gibits.leitingaar";
    private static final String GET_INSTANCE_METHOD_NAME = "getInstance";
    private static final String IS_INITED_METHOD_NAME = "isInited";

    private RemoteUtilAbcPatch() {
    }

    // //// 校验参数并执行方法级补丁 [@x380kkm 2026-08-03] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException(
                    "usage: RemoteUtilAbcPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path inputPath = Path.of(args[1]).toAbsolutePath().normalize();
        Path outputPath = Path.of(args[2]).toAbsolutePath().normalize();
        Path evidencePath = Path.of(args[3]).toAbsolutePath().normalize();
        if (inputPath.equals(outputPath)) {
            throw new IllegalArgumentException("input and output SWF paths must differ");
        }

        SWF reference = loadSwf(referencePath);
        SWF input = loadSwf(inputPath);
        RemoteUtilComparison comparison = compareRemoteUtil(reference, input);
        RemoteUtilClass referenceClass = comparison.referenceClass();
        RemoteUtilClass inputClass = comparison.inputClass();
        Map<String, String> referenceDigests = comparison.referenceDigests();
        Map<String, String> inputDigests = comparison.inputDigests();

        MethodBody requestMethod = inputClass.method(REQUEST_METHOD_NAME);
        int matchesBefore = countNativeExtensionSequences(inputClass.abc(), requestMethod);
        if (matchesBefore != 1) {
            throw new IllegalStateException(
                    "RemoteUtil native extension sequence count must be one before patch: " + matchesBefore);
        }
        patchNativeExtensionSequence(inputClass, requestMethod);
        int matchesAfter = countNativeExtensionSequences(inputClass.abc(), requestMethod);
        if (matchesAfter != 0) {
            throw new IllegalStateException(
                    "RemoteUtil native extension sequence remains after patch: " + matchesAfter);
        }

        Map<String, String> patchedDigests = RemoteUtilMethodDigest.digestStaticMethods(
                inputClass.abc(), inputClass.classIndex());
        Set<String> changedMethods = RemoteUtilMethodDigest.changedMethods(inputDigests, patchedDigests);
        if (!changedMethods.equals(Set.of(REQUEST_METHOD_NAME))) {
            throw new IllegalStateException("ABC patch changed unexpected methods: " + changedMethods);
        }
        requireDigestEqual(
                referenceDigests.get(RESPONSE_METHOD_NAME),
                patchedDigests.get(RESPONSE_METHOD_NAME),
                "requestCompleteHandler changed in memory");

        saveAndReload(
                input,
                outputPath,
                referenceDigests,
                inputDigests,
                matchesBefore,
                evidencePath);
    }
    // //// /校验参数并执行方法级补丁 ////

    // //// 验证两个 SWF 的 RemoteUtil 保持一致 [@x380kkm 2026-08-03] ////
    static void verifyRemoteUtilPreservation(Path referencePath, Path candidatePath) throws Exception {
        compareRemoteUtil(loadSwf(referencePath), loadSwf(candidatePath));
    }
    // //// /验证两个 SWF 的 RemoteUtil 保持一致 ////

    // //// 比较两个 SWF 的 RemoteUtil [@x380kkm 2026-08-03] ////
    private static RemoteUtilComparison compareRemoteUtil(SWF reference, SWF input) throws Exception {
        RemoteUtilClass referenceClass = findRemoteUtilClass(reference);
        RemoteUtilClass inputClass = findRemoteUtilClass(input);
        Map<String, String> referenceDigests = RemoteUtilMethodDigest.digestStaticMethods(
                referenceClass.abc(), referenceClass.classIndex());
        Map<String, String> inputDigests = RemoteUtilMethodDigest.digestStaticMethods(
                inputClass.abc(), inputClass.classIndex());
        RemoteUtilMethodDigest.requireSameMethods(
                referenceDigests,
                inputDigests,
                "FFDec import changed RemoteUtil before ABC patch");
        requireResponseMarkers(referenceClass.abc(), referenceClass.method(RESPONSE_METHOD_NAME));
        return new RemoteUtilComparison(referenceClass, inputClass, referenceDigests, inputDigests);
    }
    // //// /比较两个 SWF 的 RemoteUtil ////

    // //// 读取 SWF 并拒绝缺失文件 [@x380kkm 2026-08-03] ////
    private static SWF loadSwf(Path path) throws Exception {
        if (!Files.isRegularFile(path)) {
            throw new IllegalArgumentException("SWF does not exist: " + path);
        }
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF 并拒绝缺失文件 ////

    // //// 唯一定位 RemoteUtil 类 [@x380kkm 2026-08-03] ////
    private static RemoteUtilClass findRemoteUtilClass(SWF swf) {
        List<RemoteUtilClass> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(CLASS_NAME);
            if (classIndex >= 0) {
                matches.add(new RemoteUtilClass(tag, abc, classIndex));
            }
        }
        if (matches.size() != 1) {
            throw new IllegalStateException("RemoteUtil class count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /唯一定位 RemoteUtil 类 ////

    // //// 定位并替换原生扩展调用序列 [@x380kkm 2026-08-03] ////
    private static void patchNativeExtensionSequence(RemoteUtilClass remoteUtil, MethodBody body) {
        InstructionRange range = findNativeExtensionSequences(remoteUtil.abc(), body).get(0);
        body.replaceInstruction(
                range.start(),
                new AVM2Instruction(0, new PushFalseIns(), new int[0]));
        for (int index = range.end(); index > range.start(); index--) {
            body.removeInstruction(index);
        }
        body.markOffsets();
        body.setModified();
        ((Tag) remoteUtil.tag()).setModified(true);
    }
    // //// /定位并替换原生扩展调用序列 ////

    // //// 统计原生扩展调用序列 [@x380kkm 2026-08-03] ////
    private static int countNativeExtensionSequences(ABC abc, MethodBody body) {
        return findNativeExtensionSequences(abc, body).size();
    }
    // //// /统计原生扩展调用序列 ////

    // //// 查找从 getInstance 到 isInited 的连续指令 [@x380kkm 2026-08-03] ////
    private static List<InstructionRange> findNativeExtensionSequences(ABC abc, MethodBody body) {
        List<AVM2Instruction> instructions = body.getCode().code;
        List<InstructionRange> matches = new ArrayList<>();
        for (int start = 0; start < instructions.size(); start++) {
            AVM2Instruction instruction = instructions.get(start);
            if (!instruction.definition.instructionName.equals("getlex")
                    || !hasMultiname(abc, instruction, EXTENSION_NAMESPACE, EXTENSION_CLASS_NAME)) {
                continue;
            }
            int getInstance = findCall(abc, instructions, start + 1, start + 4, GET_INSTANCE_METHOD_NAME);
            int isInited = findCall(abc, instructions, getInstance + 1, start + 9, IS_INITED_METHOD_NAME);
            if (getInstance >= 0 && isInited >= 0) {
                matches.add(new InstructionRange(start, isInited));
            }
        }
        return matches;
    }
    // //// /查找从 getInstance 到 isInited 的连续指令 ////

    // //// 在有限范围内定位 callproperty [@x380kkm 2026-08-03] ////
    private static int findCall(
            ABC abc,
            List<AVM2Instruction> instructions,
            int start,
            int end,
            String methodName) {
        if (start < 0) {
            return -1;
        }
        int boundedEnd = Math.min(end, instructions.size() - 1);
        for (int index = start; index <= boundedEnd; index++) {
            AVM2Instruction instruction = instructions.get(index);
            if (instruction.definition.instructionName.equals("callproperty")
                    && hasMultiname(abc, instruction, null, methodName)) {
                return index;
            }
        }
        return -1;
    }
    // //// /在有限范围内定位 callproperty ////

    // //// 判断指令引用的 multiname [@x380kkm 2026-08-03] ////
    private static boolean hasMultiname(
            ABC abc,
            AVM2Instruction instruction,
            String expectedNamespace,
            String expectedName) {
        if (instruction.operands == null || instruction.operands.length == 0) {
            return false;
        }
        Multiname multiname = abc.constants.getMultiname(instruction.operands[0]);
        if (multiname == null || !multiname.hasOwnName()) {
            return false;
        }
        String name = abc.constants.getString(multiname.name_index);
        if (!expectedName.equals(name)) {
            return false;
        }
        if (expectedNamespace == null) {
            return true;
        }
        return expectedNamespace.equals(multiname.getSimpleNamespaceName(abc.constants).toRawString());
    }
    // //// /判断指令引用的 multiname ////

    // //// 验证原始响应方法的关键结构 [@x380kkm 2026-08-03] ////
    private static void requireResponseMarkers(ABC abc, MethodBody body) {
        Map<String, Set<String>> markerInstructions = Map.of(
                "result_code", Set.of("getproperty", "getlex", "findpropstrict", "findproperty"),
                "ResponseData", Set.of("constructprop", "coerce", "getlex", "findpropstrict"),
                "isSuccess", Set.of("callproperty", "callpropvoid"));
        for (Map.Entry<String, Set<String>> marker : markerInstructions.entrySet()) {
            boolean found = body.getCode().code.stream()
                    .anyMatch(instruction -> marker.getValue().contains(instruction.definition.instructionName)
                            && hasMultiname(abc, instruction, null, marker.getKey()));
            if (!found) {
                throw new IllegalStateException("requestCompleteHandler is missing marker: " + marker.getKey());
            }
        }
    }
    // //// /验证原始响应方法的关键结构 ////

    // //// 保存 SWF 并复读全部方法摘要 [@x380kkm 2026-08-03] ////
    private static void saveAndReload(
            SWF swf,
            Path outputPath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            int matchesBefore,
            Path evidencePath) throws Exception {
        Files.createDirectories(outputPath.getParent());
        Path temporaryPath = outputPath.resolveSibling(outputPath.getFileName() + ".tmp");
        Files.deleteIfExists(temporaryPath);
        try (OutputStream output = Files.newOutputStream(temporaryPath)) {
            swf.saveTo(output);
        }

        SWF reloaded = loadSwf(temporaryPath);
        RemoteUtilClass reloadedClass = findRemoteUtilClass(reloaded);
        Map<String, String> outputDigests = RemoteUtilMethodDigest.digestStaticMethods(
                reloadedClass.abc(), reloadedClass.classIndex());
        Set<String> changedMethods = RemoteUtilMethodDigest.changedMethods(inputDigests, outputDigests);
        if (!changedMethods.equals(Set.of(REQUEST_METHOD_NAME))) {
            throw new IllegalStateException("saved SWF changed unexpected methods: " + changedMethods);
        }
        requireDigestEqual(
                referenceDigests.get(RESPONSE_METHOD_NAME),
                outputDigests.get(RESPONSE_METHOD_NAME),
                "saved requestCompleteHandler differs from original");
        int matchesAfter = countNativeExtensionSequences(
                reloadedClass.abc(),
                reloadedClass.method(REQUEST_METHOD_NAME));
        if (matchesAfter != 0) {
            throw new IllegalStateException("saved SWF retains native extension sequence: " + matchesAfter);
        }

        moveAtomically(temporaryPath, outputPath);
        writeEvidence(
                evidencePath,
                referenceDigests,
                inputDigests,
                outputDigests,
                matchesBefore,
                matchesAfter,
                changedMethods);
    }
    // //// /保存 SWF 并复读全部方法摘要 ////

    // //// 原子替换输出文件 [@x380kkm 2026-08-03] ////
    private static void moveAtomically(Path source, Path target) throws Exception {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException error) {
            Files.move(source, target);
        }
    }
    // //// /原子替换输出文件 ////

    // //// 写入不含客户端数据的完整性证据 [@x380kkm 2026-08-03] ////
    private static void writeEvidence(
            Path evidencePath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            Map<String, String> outputDigests,
            int matchesBefore,
            int matchesAfter,
            Set<String> changedMethods) throws Exception {
        Files.createDirectories(evidencePath.getParent());
        String changed = changedMethods.stream()
                .map(RemoteUtilAbcPatch::jsonString)
                .reduce((left, right) -> left + "," + right)
                .orElse("");
        String json = "{\n"
                + "  \"className\": " + jsonString(CLASS_NAME) + ",\n"
                + "  \"patchMethod\": " + jsonString(REQUEST_METHOD_NAME) + ",\n"
                + "  \"digestVersion\": " + RemoteUtilMethodDigest.VERSION + ",\n"
                + "  \"digestIncludes\": [\"method-info\",\"method-body\",\"constant-semantics\",\"exceptions\",\"traits\"],\n"
                + "  \"methodOnly\": true,\n"
                + "  \"unchangedMethodsVerified\": true,\n"
                + "  \"changedMethods\": [" + changed + "],\n"
                + "  \"nativeExtensionSequenceCountBefore\": " + matchesBefore + ",\n"
                + "  \"nativeExtensionSequenceCountAfter\": " + matchesAfter + ",\n"
                + "  \"requestCompleteHandler\": {\n"
                + "    \"referenceSha256\": " + jsonString(referenceDigests.get(RESPONSE_METHOD_NAME)) + ",\n"
                + "    \"inputSha256\": " + jsonString(inputDigests.get(RESPONSE_METHOD_NAME)) + ",\n"
                + "    \"outputSha256\": " + jsonString(outputDigests.get(RESPONSE_METHOD_NAME)) + "\n"
                + "  },\n"
                + "  \"getURLRequest\": {\n"
                + "    \"inputSha256\": " + jsonString(inputDigests.get(REQUEST_METHOD_NAME)) + ",\n"
                + "    \"outputSha256\": " + jsonString(outputDigests.get(REQUEST_METHOD_NAME)) + "\n"
                + "  }\n"
                + "}\n";
        Files.writeString(evidencePath, json, StandardCharsets.UTF_8);
    }
    // //// /写入不含客户端数据的完整性证据 ////

    // //// 要求两个方法摘要一致 [@x380kkm 2026-08-03] ////
    private static void requireDigestEqual(String expected, String actual, String message) {
        if (!expected.equals(actual)) {
            throw new IllegalStateException(message + ": expected=" + expected + " actual=" + actual);
        }
    }
    // //// /要求两个方法摘要一致 ////

    // //// 编码 JSON 字符串 [@x380kkm 2026-08-03] ////
    private static String jsonString(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }
    // //// /编码 JSON 字符串 ////

    private record InstructionRange(int start, int end) {
    }

    private record RemoteUtilComparison(
            RemoteUtilClass referenceClass,
            RemoteUtilClass inputClass,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests) {
    }

    private record RemoteUtilClass(ABCContainerTag tag, ABC abc, int classIndex) {
        // //// 读取 RemoteUtil 的指定静态方法 [@x380kkm 2026-08-03] ////
        private MethodBody method(String methodName) {
            return RemoteUtilMethodDigest.findStaticMethod(abc, classIndex, methodName);
        }
        // //// /读取 RemoteUtil 的指定静态方法 ////
    }
}
