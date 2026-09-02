// audience: internal
// # remote-util-decode-diagnostic-patch
// 此工具在 RemoteUtil ParseError 文本中追加响应长度, 分段 SHA-256 和 MessagePack 解码位置, 并强制 DisplayableError 显示内部错误文本.
// 运行前提是 Java 17 和 Starview 随附的 FFDec 库.
// 工具拒绝输入目标方法已被改写, 也拒绝输出修改未声明的方法.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.arithmetic.AddIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.executing.CallPropertyIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.localregs.GetLocalIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.GetLexIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.GetPropertyIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushShortIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushStringIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushTrueIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.types.CoerceIns;
import com.jpexs.decompiler.flash.abc.types.InstanceInfo;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.abc.types.Namespace;
import com.jpexs.decompiler.flash.abc.types.traits.Trait;
import com.jpexs.decompiler.flash.abc.types.traits.TraitFunction;
import com.jpexs.decompiler.flash.abc.types.traits.TraitMethodGetterSetter;
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

public final class RemoteUtilDecodeDiagnosticPatch {
    private static final String CLASS_NAME = "pinball.context.remote.RemoteUtil";
    private static final String REQUEST_METHOD_NAME = "getURLRequest";
    private static final String RESPONSE_METHOD_NAME = "requestCompleteHandler";
    private static final String DISPLAY_CLASS_NAME = "pinball.common.error.DisplayableError";
    private static final String DISPLAY_METHOD_NAME = "getDisplayMessage";
    private static final String BASE64_CLASS_NAME = "haxe.crypto.Base64";
    private static final String SHA256_CLASS_NAME = "haxe.crypto.Sha256";
    private static final String BYTES_CLASS_NAME = "haxe.io.Bytes";

    private static final int URL_LOADER_LOCAL = 6;
    private static final int BYTES_LOCAL = 18;
    private static final int DECODER_LOCAL = 19;
    private static final int DIGEST_PREFIX_LENGTH = 8;

    private RemoteUtilDecodeDiagnosticPatch() {
    }

    // //// 校验参数并执行响应错误文本补丁 [@x380kkm 2026-08-10] ////
    public static void main(String[] arguments) throws Exception {
        if (arguments.length != 4) {
            throw new IllegalArgumentException(
                    "usage: RemoteUtilDecodeDiagnosticPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }
        Path referencePath = normalizedPath(arguments[0]);
        Path inputPath = normalizedPath(arguments[1]);
        Path outputPath = normalizedPath(arguments[2]);
        Path evidencePath = normalizedPath(arguments[3]);
        if (inputPath.equals(outputPath)) {
            throw new IllegalArgumentException("input and output SWF paths must differ");
        }
        patch(referencePath, inputPath, outputPath, evidencePath);
    }
    // //// /校验参数并执行响应错误文本补丁 ////

    // //// 对指定 SWF 应用诊断补丁并复读验证 [@x380kkm 2026-08-10] ////
    static void patch(Path referencePath, Path inputPath, Path outputPath, Path evidencePath) throws Exception {
        SWF reference = loadSwf(referencePath);
        SWF input = loadSwf(inputPath);
        RemoteUtilClass referenceClass = findRemoteUtilClass(reference);
        RemoteUtilClass inputClass = findRemoteUtilClass(input);
        DisplayableErrorClass referenceDisplayClass = findDisplayableErrorClass(reference);
        DisplayableErrorClass inputDisplayClass = findDisplayableErrorClass(input);
        Map<String, String> referenceDigests = RemoteUtilMethodDigest.digestStaticMethods(
                referenceClass.abc(), referenceClass.classIndex());
        Map<String, String> inputDigests = RemoteUtilMethodDigest.digestStaticMethods(
                inputClass.abc(), inputClass.classIndex());
        Set<String> inputChanges = RemoteUtilMethodDigest.changedMethods(referenceDigests, inputDigests);
        if (!inputChanges.isEmpty() && !inputChanges.equals(Set.of(REQUEST_METHOD_NAME))) {
            throw new IllegalStateException(
                    "input RemoteUtil contains unexpected changes: " + inputChanges);
        }
        requireEqual(
                referenceDigests.get(RESPONSE_METHOD_NAME),
                inputDigests.get(RESPONSE_METHOD_NAME),
                "input requestCompleteHandler differs from reference");
        String referenceDisplayDigest = digestDisplayMethod(referenceDisplayClass);
        String inputDisplayDigest = digestDisplayMethod(inputDisplayClass);
        requireEqual(
                referenceDisplayDigest,
                inputDisplayDigest,
                "input DisplayableError.getDisplayMessage differs from reference");

        MethodBody responseMethod = inputClass.method(RESPONSE_METHOD_NAME);
        int match = findParseErrorStringSequence(inputClass.abc(), responseMethod);
        if (match < 0) {
            throw new IllegalStateException("requestCompleteHandler ParseError sequence is not unique");
        }
        insertDecodeContext(inputClass.abc(), responseMethod, match);
        responseMethod.markOffsets();
        responseMethod.setModified();
        ((Tag) inputClass.tag()).setModified(true);

        InstanceMethod displayMethod = inputDisplayClass.method();
        int displayMatch = forceDisplayableErrorMessage(displayMethod.body());
        displayMethod.body().markOffsets();
        displayMethod.body().setModified();
        ((Tag) inputDisplayClass.tag()).setModified(true);

        Map<String, String> memoryDigests = RemoteUtilMethodDigest.digestStaticMethods(
                inputClass.abc(), inputClass.classIndex());
        Set<String> memoryChanges = RemoteUtilMethodDigest.changedMethods(inputDigests, memoryDigests);
        requireOnlyResponseChange(memoryChanges, "in-memory diagnostic patch");
        String memoryDisplayDigest = digestDisplayMethod(inputDisplayClass);
        if (memoryDisplayDigest.equals(inputDisplayDigest)) {
            throw new IllegalStateException("in-memory diagnostic patch did not change DisplayableError.getDisplayMessage");
        }
        saveAndReload(
                input,
                outputPath,
                referenceDigests,
                inputDigests,
                inputChanges,
                match,
                displayMatch,
                referenceDisplayDigest,
                inputDisplayDigest,
                evidencePath);
    }
    // //// /对指定 SWF 应用诊断补丁并复读验证 ////

    // //// 读取 SWF 并拒绝缺失文件 [@x380kkm 2026-08-10] ////
    private static SWF loadSwf(Path path) throws Exception {
        if (!Files.isRegularFile(path)) {
            throw new IllegalArgumentException("SWF does not exist: " + path);
        }
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF 并拒绝缺失文件 ////

    // //// 唯一定位 RemoteUtil 类 [@x380kkm 2026-08-10] ////
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

    // //// 唯一定位 DisplayableError 类 [@x380kkm 2026-08-11] ////
    private static DisplayableErrorClass findDisplayableErrorClass(SWF swf) {
        List<DisplayableErrorClass> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(DISPLAY_CLASS_NAME);
            if (classIndex >= 0) {
                matches.add(new DisplayableErrorClass(tag, abc, classIndex));
            }
        }
        if (matches.size() != 1) {
            throw new IllegalStateException("DisplayableError class count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /唯一定位 DisplayableError 类 ////

    // //// 查找 DisplayableError 内部错误显示方法 [@x380kkm 2026-08-11] ////
    private static InstanceMethod findInstanceMethod(ABC abc, int classIndex, String methodName) {
        InstanceInfo instance = abc.instance_info.get(classIndex);
        for (Trait trait : instance.instance_traits.traits) {
            Integer methodInfo = traitMethodInfo(trait);
            if (methodInfo == null) {
                continue;
            }
            Multiname name = abc.constants.getMultiname(trait.name_index);
            if (name == null || !name.hasOwnName()
                    || !methodName.equals(abc.constants.getString(name.name_index))) {
                continue;
            }
            MethodBody body = abc.findBody(methodInfo);
            if (body != null) {
                return new InstanceMethod(trait, body);
            }
        }
        throw new IllegalStateException("DisplayableError method is missing: " + methodName);
    }
    // //// /查找 DisplayableError 内部错误显示方法 ////

    // //// 计算 DisplayableError 方法摘要 [@x380kkm 2026-08-11] ////
    private static String digestDisplayMethod(DisplayableErrorClass displayClass) throws Exception {
        InstanceMethod method = displayClass.method();
        return RemoteUtilMethodDigest.digestMethod(displayClass.abc(), method.trait(), method.body());
    }
    // //// /计算 DisplayableError 方法摘要 ////

    // //// 强制内部错误消息显示并返回修改位置 [@x380kkm 2026-08-11] ////
    private static int forceDisplayableErrorMessage(MethodBody body) {
        List<AVM2Instruction> instructions = body.getCode().code;
        int match = -1;
        for (int index = 0; index + 1 < instructions.size(); index++) {
            if (!isGetLocal(instructions.get(index), 1)
                    || !instructions.get(index + 1).definition.instructionName.equals("iffalse")) {
                continue;
            }
            if (match >= 0) {
                throw new IllegalStateException("DisplayableError.getDisplayMessage condition is not unique");
            }
            match = index;
        }
        if (match < 0) {
            throw new IllegalStateException("DisplayableError.getDisplayMessage condition is missing");
        }
        body.replaceInstruction(match, new AVM2Instruction(0, new PushTrueIns(), new int[0]));
        return match;
    }
    // //// /强制内部错误消息显示并返回修改位置 ////

    // //// 查找唯一 ParseError 字符串构造序列 [@x380kkm 2026-08-10] ////
    private static int findParseErrorStringSequence(ABC abc, MethodBody body) {
        List<AVM2Instruction> instructions = body.getCode().code;
        int match = -1;
        for (int index = 0; index + 4 < instructions.size(); index++) {
            if (!hasInstruction(instructions.get(index), "getlex", abc, "Std")
                    || !isGetLocal(instructions.get(index + 1), 20)
                    || !hasInstruction(instructions.get(index + 2), "callproperty", abc, "string")
                    || !hasInstruction(instructions.get(index + 3), "coerce", abc, "String")
                    || !hasInstruction(instructions.get(index + 4), "callproperty", abc, "ParseError")) {
                continue;
            }
            if (match >= 0) {
                return -1;
            }
            match = index;
        }
        return match;
    }
    // //// /查找唯一 ParseError 字符串构造序列 ////

    // //// 在错误文本后追加响应文本和 MessagePack 解码状态 [@x380kkm 2026-08-11] ////
    private static void insertDecodeContext(ABC abc, MethodBody body, int sequenceStart) {
        int insertIndex = sequenceStart + 4;
        int data = publicName(abc, "data");
        int length = publicName(abc, "length");
        int decoderInput = publicName(abc, "i");
        int inputBytes = publicName(abc, "b");
        int position = publicName(abc, "position");
        int bytesAvailable = publicName(abc, "bytesAvailable");
        List<AVM2Instruction> additions = new ArrayList<>();

        appendString(additions, abc, " chars=");
        append(additions, new AddIns());
        appendLocalProperty(additions, URL_LOADER_LOCAL, data);
        appendProperty(additions, length);
        append(additions, new AddIns());
        appendString(additions, abc, " text_sha256=");
        append(additions, new AddIns());
        appendTextSha256(additions, abc, data);
        append(additions, new AddIns());
        appendString(additions, abc, " t4=");
        append(additions, new AddIns());
        appendTextPrefixSha256(additions, abc, data, 4096);
        append(additions, new AddIns());
        appendString(additions, abc, " t8=");
        append(additions, new AddIns());
        appendTextPrefixSha256(additions, abc, data, 8192);
        append(additions, new AddIns());
        appendString(additions, abc, " t12=");
        append(additions, new AddIns());
        appendTextPrefixSha256(additions, abc, data, 12288);
        append(additions, new AddIns());
        appendString(additions, abc, " bytes=");
        append(additions, new AddIns());
        appendLocalProperty(additions, BYTES_LOCAL, length);
        append(additions, new AddIns());
        appendString(additions, abc, " decoded_sha256=");
        append(additions, new AddIns());
        appendBytesSha256(additions, abc);
        append(additions, new AddIns());
        appendString(additions, abc, " pos=");
        append(additions, new AddIns());
        appendLocalProperty(additions, DECODER_LOCAL, decoderInput);
        appendProperty(additions, inputBytes);
        appendProperty(additions, position);
        append(additions, new AddIns());
        appendString(additions, abc, " remaining=");
        append(additions, new AddIns());
        appendLocalProperty(additions, DECODER_LOCAL, decoderInput);
        appendProperty(additions, inputBytes);
        appendProperty(additions, bytesAvailable);
        append(additions, new AddIns());
        body.insertAll(insertIndex, additions);
    }
    // //// /在错误文本后追加响应文本和 MessagePack 解码状态 ////

    private static void appendTextSha256(
            List<AVM2Instruction> instructions,
            ABC abc,
            int dataProperty) {
        append(instructions, new GetLexIns(), qualifiedName(abc, BASE64_CLASS_NAME));
        append(instructions, new GetLexIns(), qualifiedName(abc, SHA256_CLASS_NAME));
        append(instructions, new GetLexIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        appendLocalProperty(instructions, URL_LOADER_LOCAL, dataProperty);
        append(instructions, new CallPropertyIns(), publicName(abc, "ofString"), 1);
        append(instructions, new CoerceIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        append(instructions, new CallPropertyIns(), publicName(abc, "make"), 1);
        append(instructions, new CoerceIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        append(instructions, new CallPropertyIns(), publicName(abc, "encode"), 1);
        append(instructions, new CoerceIns(), publicName(abc, "String"));
    }

    private static void appendTextPrefixSha256(
            List<AVM2Instruction> instructions,
            ABC abc,
            int dataProperty,
            int prefixLength) {
        append(instructions, new GetLexIns(), qualifiedName(abc, BASE64_CLASS_NAME));
        append(instructions, new GetLexIns(), qualifiedName(abc, SHA256_CLASS_NAME));
        append(instructions, new GetLexIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        appendLocalProperty(instructions, URL_LOADER_LOCAL, dataProperty);
        appendInteger(instructions, 0);
        appendInteger(instructions, prefixLength);
        append(instructions, new CallPropertyIns(), publicName(abc, "substr"), 2);
        append(instructions, new CoerceIns(), publicName(abc, "String"));
        append(instructions, new CallPropertyIns(), publicName(abc, "ofString"), 1);
        append(instructions, new CoerceIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        append(instructions, new CallPropertyIns(), publicName(abc, "make"), 1);
        append(instructions, new CoerceIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        append(instructions, new CallPropertyIns(), publicName(abc, "encode"), 1);
        append(instructions, new CoerceIns(), publicName(abc, "String"));
        appendInteger(instructions, 0);
        appendInteger(instructions, DIGEST_PREFIX_LENGTH);
        append(instructions, new CallPropertyIns(), publicName(abc, "substr"), 2);
        append(instructions, new CoerceIns(), publicName(abc, "String"));
    }

    private static void appendBytesSha256(List<AVM2Instruction> instructions, ABC abc) {
        append(instructions, new GetLexIns(), qualifiedName(abc, BASE64_CLASS_NAME));
        append(instructions, new GetLexIns(), qualifiedName(abc, SHA256_CLASS_NAME));
        append(instructions, new GetLocalIns(), BYTES_LOCAL);
        append(instructions, new CallPropertyIns(), publicName(abc, "make"), 1);
        append(instructions, new CoerceIns(), qualifiedName(abc, BYTES_CLASS_NAME));
        append(instructions, new CallPropertyIns(), publicName(abc, "encode"), 1);
        append(instructions, new CoerceIns(), publicName(abc, "String"));
    }

    // //// 生成公共属性名称索引 [@x380kkm 2026-08-10] ////
    private static int publicName(ABC abc, String name) {
        return abc.constants.getPublicQnameId(name, true);
    }
    // //// /生成公共属性名称索引 ////

    private static int qualifiedName(ABC abc, String className) {
        int separator = className.lastIndexOf('.');
        if (separator <= 0 || separator == className.length() - 1) {
            throw new IllegalArgumentException("class name must include a package: " + className);
        }
        return abc.constants.getQnameId(
                className.substring(separator + 1),
                Namespace.KIND_PACKAGE,
                className.substring(0, separator),
                true);
    }

    private static void appendString(List<AVM2Instruction> instructions, ABC abc, String value) {
        append(instructions, new PushStringIns(), abc.constants.addString(value));
    }

    private static void appendInteger(List<AVM2Instruction> instructions, int value) {
        append(instructions, new PushShortIns(), value);
    }

    private static void appendLocalProperty(
            List<AVM2Instruction> instructions,
            int local,
            int property) {
        append(instructions, new GetLocalIns(), local);
        appendProperty(instructions, property);
    }

    private static void appendProperty(List<AVM2Instruction> instructions, int property) {
        append(instructions, new GetPropertyIns(), property);
    }

    private static void append(List<AVM2Instruction> instructions,
            com.jpexs.decompiler.flash.abc.avm2.instructions.InstructionDefinition definition,
            int... operands) {
        instructions.add(new AVM2Instruction(0, definition, operands));
    }

    private static void append(
            List<AVM2Instruction> instructions,
            com.jpexs.decompiler.flash.abc.avm2.instructions.InstructionDefinition definition) {
        append(instructions, definition, new int[0]);
    }

    private static Integer traitMethodInfo(Trait trait) {
        if (trait instanceof TraitMethodGetterSetter method) {
            return method.method_info;
        }
        if (trait instanceof TraitFunction function) {
            return function.method_info;
        }
        return null;
    }

    private static boolean isGetLocal(AVM2Instruction instruction, int register) {
        String name = instruction.definition.instructionName;
        if (name.equals("getlocal")
                && instruction.operands != null
                && instruction.operands.length == 1) {
            return instruction.operands[0] == register;
        }
        return register >= 0 && register <= 3
                && (name.equals("getlocal" + register) || name.equals("getlocal_" + register));
    }

    private static boolean hasInstruction(
            AVM2Instruction instruction,
            String instructionName,
            ABC abc,
            String name) {
        if (!instruction.definition.instructionName.equals(instructionName)
                || instruction.operands == null
                || instruction.operands.length == 0) {
            return false;
        }
        Multiname multiname = abc.constants.getMultiname(instruction.operands[0]);
        return multiname != null
                && multiname.hasOwnName()
                && name.equals(abc.constants.getString(multiname.name_index));
    }

    // //// 保存 SWF 并验证两个诊断方法改变 [@x380kkm 2026-08-11] ////
    private static void saveAndReload(
            SWF swf,
            Path outputPath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            Set<String> inputChanges,
            int sequenceStart,
            int displayConditionIndex,
            String referenceDisplayDigest,
            String inputDisplayDigest,
            Path evidencePath) throws Exception {
        Files.createDirectories(outputPath.toAbsolutePath().normalize().getParent());
        Path temporaryPath = outputPath.resolveSibling(outputPath.getFileName() + ".tmp");
        Files.deleteIfExists(temporaryPath);
        try (OutputStream output = Files.newOutputStream(temporaryPath)) {
            swf.saveTo(output);
        }

        SWF reloaded = loadSwf(temporaryPath);
        RemoteUtilClass outputClass = findRemoteUtilClass(reloaded);
        DisplayableErrorClass outputDisplayClass = findDisplayableErrorClass(reloaded);
        Map<String, String> outputDigests = RemoteUtilMethodDigest.digestStaticMethods(
                outputClass.abc(), outputClass.classIndex());
        Set<String> outputChanges = RemoteUtilMethodDigest.changedMethods(inputDigests, outputDigests);
        requireOnlyResponseChange(outputChanges, "saved diagnostic patch");
        requireEqual(
                inputDigests.get(RESPONSE_METHOD_NAME),
                referenceDigests.get(RESPONSE_METHOD_NAME),
                "input response digest must retain the reference relation");
        if (outputDigests.get(RESPONSE_METHOD_NAME).equals(inputDigests.get(RESPONSE_METHOD_NAME))) {
            throw new IllegalStateException("diagnostic patch did not change requestCompleteHandler");
        }
        if (outputDigests.get(REQUEST_METHOD_NAME) == null
                || !outputDigests.get(REQUEST_METHOD_NAME).equals(inputDigests.get(REQUEST_METHOD_NAME))) {
            throw new IllegalStateException("diagnostic patch changed getURLRequest");
        }
        String outputDisplayDigest = digestDisplayMethod(outputDisplayClass);
        requireEqual(
                referenceDisplayDigest,
                inputDisplayDigest,
                "input DisplayableError digest must retain the reference relation");
        if (outputDisplayDigest.equals(inputDisplayDigest)) {
            throw new IllegalStateException("diagnostic patch did not change DisplayableError.getDisplayMessage");
        }

        moveAtomically(temporaryPath, outputPath);
        writeEvidence(
                evidencePath,
                referenceDigests,
                inputDigests,
                outputDigests,
                inputChanges,
                outputChanges,
                sequenceStart,
                displayConditionIndex,
                referenceDisplayDigest,
                inputDisplayDigest,
                outputDisplayDigest);
    }
    // //// /保存 SWF 并验证两个诊断方法改变 ////

    private static void requireOnlyResponseChange(Set<String> changes, String label) {
        if (!changes.equals(Set.of(RESPONSE_METHOD_NAME))) {
            throw new IllegalStateException(label + " changed methods: " + changes);
        }
    }

    private static void requireEqual(String expected, String actual, String message) {
        if (expected == null || !expected.equals(actual)) {
            throw new IllegalStateException(message + ": expected=" + expected + " actual=" + actual);
        }
    }

    private static void moveAtomically(Path source, Path target) throws Exception {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException error) {
            Files.move(source, target);
        }
    }

    // //// 写入不含客户端数据的诊断证据 [@x380kkm 2026-08-10] ////
    private static void writeEvidence(
            Path evidencePath,
            Map<String, String> referenceDigests,
            Map<String, String> inputDigests,
            Map<String, String> outputDigests,
            Set<String> inputChanges,
            Set<String> outputChanges,
            int sequenceStart,
            int displayConditionIndex,
            String referenceDisplayDigest,
            String inputDisplayDigest,
            String outputDisplayDigest) throws Exception {
        Files.createDirectories(evidencePath.toAbsolutePath().normalize().getParent());
        String json = "{\n"
                + "  \"className\": \"" + CLASS_NAME + "\",\n"
                + "  \"patchMethod\": \"" + RESPONSE_METHOD_NAME + "\",\n"
                + "  \"digestVersion\": " + RemoteUtilMethodDigest.VERSION + ",\n"
                + "  \"methodOnly\": true,\n"
                + "  \"changesErrorTextOnly\": true,\n"
                + "  \"forcesInternalErrorDisplay\": true,\n"
                + "  \"diagnosticFields\": [\"responseTextLength\",\"responseTextSha256\",\"responseTextPrefix4096Sha256Prefix\",\"responseTextPrefix8192Sha256Prefix\",\"responseTextPrefix12288Sha256Prefix\",\"decodedBytesLength\",\"decodedBytesSha256\",\"decoderPosition\",\"decoderBytesAvailable\"],\n"
                + "  \"sequenceStart\": " + sequenceStart + ",\n"
                + "  \"displayConditionIndex\": " + displayConditionIndex + ",\n"
                + "  \"inputChangesFromReference\": " + jsonArray(inputChanges) + ",\n"
                + "  \"changedMethods\": " + jsonArray(outputChanges) + ",\n"
                + "  \"requestCompleteHandler\": {\n"
                + "    \"referenceSha256\": \"" + referenceDigests.get(RESPONSE_METHOD_NAME) + "\",\n"
                + "    \"inputSha256\": \"" + inputDigests.get(RESPONSE_METHOD_NAME) + "\",\n"
                + "    \"outputSha256\": \"" + outputDigests.get(RESPONSE_METHOD_NAME) + "\"\n"
                + "  },\n"
                + "  \"getURLRequest\": {\n"
                + "    \"inputSha256\": \"" + inputDigests.get(REQUEST_METHOD_NAME) + "\",\n"
                + "    \"outputSha256\": \"" + outputDigests.get(REQUEST_METHOD_NAME) + "\"\n"
                + "  },\n"
                + "  \"displayableError\": {\n"
                + "    \"className\": \"" + DISPLAY_CLASS_NAME + "\",\n"
                + "    \"patchMethod\": \"" + DISPLAY_METHOD_NAME + "\",\n"
                + "    \"referenceSha256\": \"" + referenceDisplayDigest + "\",\n"
                + "    \"inputSha256\": \"" + inputDisplayDigest + "\",\n"
                + "    \"outputSha256\": \"" + outputDisplayDigest + "\"\n"
                + "  }\n"
                + "}\n";
        Files.writeString(evidencePath, json, StandardCharsets.UTF_8);
    }
    // //// /写入不含客户端数据的诊断证据 ////

    private static String jsonArray(Set<String> values) {
        return values.stream()
                .sorted()
                .map(value -> "\"" + value + "\"")
                .reduce((left, right) -> left + "," + right)
                .map(value -> "[" + value + "]")
                .orElse("[]");
    }

    private static Path normalizedPath(String value) {
        return Path.of(value).toAbsolutePath().normalize();
    }

    private record RemoteUtilClass(ABCContainerTag tag, ABC abc, int classIndex) {
        private MethodBody method(String methodName) {
            return RemoteUtilMethodDigest.findStaticMethod(abc, classIndex, methodName);
        }
    }

    private record DisplayableErrorClass(ABCContainerTag tag, ABC abc, int classIndex) {
        private InstanceMethod method() {
            return findInstanceMethod(abc, classIndex, DISPLAY_METHOD_NAME);
        }
    }

    private record InstanceMethod(Trait trait, MethodBody body) {
    }
}
