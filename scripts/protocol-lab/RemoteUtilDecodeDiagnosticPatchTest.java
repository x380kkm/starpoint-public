// audience: internal
// # remote-util-decode-diagnostic-patch-test
// 此测试使用外部 CN SWF 验证解码诊断只改变响应方法并且不能重复应用.
// 运行前提是 Java 17, Starview 随附的 FFDec 库和未提交的 CN 客户端 SWF.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.types.InstanceInfo;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.abc.types.traits.Trait;
import com.jpexs.decompiler.flash.abc.types.traits.TraitFunction;
import com.jpexs.decompiler.flash.abc.types.traits.TraitMethodGetterSetter;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;

import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class RemoteUtilDecodeDiagnosticPatchTest {
    private static final String CLASS_NAME = "pinball.context.remote.RemoteUtil";
    private static final String RESPONSE_METHOD_NAME = "requestCompleteHandler";
    private static final String REQUEST_METHOD_NAME = "getURLRequest";
    private static final String DISPLAY_CLASS_NAME = "pinball.common.error.DisplayableError";
    private static final String DISPLAY_METHOD_NAME = "getDisplayMessage";

    private RemoteUtilDecodeDiagnosticPatchTest() {
    }

    // //// 运行真实 SWF 解码诊断回归测试 [@x380kkm 2026-08-10] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: RemoteUtilDecodeDiagnosticPatchTest <reference.swf> <temporary-directory>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path temporaryDirectory = Path.of(args[1]).toAbsolutePath().normalize();
        Files.createDirectories(temporaryDirectory);

        Path patchedPath = temporaryDirectory.resolve("diagnostic.swf");
        Path evidencePath = temporaryDirectory.resolve("diagnostic.json");
        RemoteUtilDecodeDiagnosticPatch.patch(referencePath, referencePath, patchedPath, evidencePath);

        verifyPatchedMethods(referencePath, patchedPath);
        verifyDiagnosticInstructions(patchedPath);
        verifyDisplayMethod(referencePath, patchedPath);
        verifyEvidence(referencePath, patchedPath, evidencePath);
        verifyRepeatedPatchIsRejected(referencePath, patchedPath, temporaryDirectory.resolve("repeated.swf"));
        System.out.println("RemoteUtilDecodeDiagnosticPatchTest: PASS");
    }
    // //// /运行真实 SWF 解码诊断回归测试 ////

    // //// 验证输出只改变响应方法 [@x380kkm 2026-08-10] ////
    private static void verifyPatchedMethods(Path referencePath, Path patchedPath) throws Exception {
        RemoteUtilFixture reference = findRemoteUtil(loadSwf(referencePath));
        RemoteUtilFixture patched = findRemoteUtil(loadSwf(patchedPath));
        Map<String, String> referenceDigests = RemoteUtilMethodDigest.digestStaticMethods(
                reference.abc(), reference.classIndex());
        Map<String, String> patchedDigests = RemoteUtilMethodDigest.digestStaticMethods(
                patched.abc(), patched.classIndex());
        Set<String> changedMethods = RemoteUtilMethodDigest.changedMethods(referenceDigests, patchedDigests);
        require(changedMethods.equals(Set.of(RESPONSE_METHOD_NAME)),
                "解码诊断输出改变了未声明方法: " + changedMethods);
        require(referenceDigests.get(REQUEST_METHOD_NAME).equals(patchedDigests.get(REQUEST_METHOD_NAME)),
                "解码诊断输出改变了 getURLRequest");
        require(!referenceDigests.get(RESPONSE_METHOD_NAME).equals(patchedDigests.get(RESPONSE_METHOD_NAME)),
                "解码诊断输出没有改变 requestCompleteHandler");
    }
    // //// /验证输出只改变响应方法 ////

    // //// 验证响应文本和解码字节摘要指令 [@x380kkm 2026-08-11] ////
    private static void verifyDiagnosticInstructions(Path patchedPath) throws Exception {
        RemoteUtilFixture patched = findRemoteUtil(loadSwf(patchedPath));
        MethodBody body = RemoteUtilMethodDigest.findStaticMethod(
                patched.abc(), patched.classIndex(), RESPONSE_METHOD_NAME);

        require(countReferencedInstructions(patched.abc(), body, "getlex", "Sha256") == 5,
                "解码诊断没有计算完整响应文本, 三个响应前缀和解码字节的 SHA-256");
        require(countReferencedInstructions(patched.abc(), body, "callproperty", "ofString") == 4,
                "解码诊断没有把完整响应文本和三个响应前缀转换为 UTF-8 字节");
        require(countReferencedInstructions(patched.abc(), body, "callproperty", "make") == 5,
                "解码诊断的 SHA-256 调用数量不正确");
        require(countReferencedInstructions(patched.abc(), body, "callproperty", "encode") == 5,
                "解码诊断的 Base64 摘要调用数量不正确");
        require(countReferencedInstructions(patched.abc(), body, "callproperty", "substr") == 6,
                "解码诊断没有提取三个响应前缀和三个摘要前缀");
        require(countPushedString(patched.abc(), body, " chars=") == 1,
                "解码诊断没有显示 URLLoader 文本长度");
        require(countPushedString(patched.abc(), body, " text_sha256=") == 1,
                "解码诊断没有显示 URLLoader 文本摘要");
        require(countPushedString(patched.abc(), body, " t4=") == 1,
                "解码诊断没有显示 4096 字符前缀摘要");
        require(countPushedString(patched.abc(), body, " t8=") == 1,
                "解码诊断没有显示 8192 字符前缀摘要");
        require(countPushedString(patched.abc(), body, " t12=") == 1,
                "解码诊断没有显示 12288 字符前缀摘要");
        require(countPushedString(patched.abc(), body, " decoded_sha256=") == 1,
                "解码诊断没有显示解码字节摘要");
    }
    // //// /验证响应文本和解码字节摘要指令 ////

    // //// 验证内部错误显示方法发生唯一声明变化 [@x380kkm 2026-08-11] ////
    private static void verifyDisplayMethod(Path referencePath, Path patchedPath) throws Exception {
        String referenceDigest = digestDisplayMethod(loadSwf(referencePath));
        String patchedDigest = digestDisplayMethod(loadSwf(patchedPath));
        require(!referenceDigest.equals(patchedDigest),
                "解码诊断输出没有改变 DisplayableError.getDisplayMessage");
    }
    // //// /验证内部错误显示方法发生唯一声明变化 ////

    // //// 验证诊断证据字段与方法摘要一致 [@x380kkm 2026-08-10] ////
    private static void verifyEvidence(Path referencePath, Path patchedPath, Path evidencePath) throws Exception {
        String evidence = Files.readString(evidencePath, StandardCharsets.UTF_8);
        RemoteUtilFixture reference = findRemoteUtil(loadSwf(referencePath));
        RemoteUtilFixture patched = findRemoteUtil(loadSwf(patchedPath));
        Map<String, String> referenceDigests = RemoteUtilMethodDigest.digestStaticMethods(
                reference.abc(), reference.classIndex());
        Map<String, String> patchedDigests = RemoteUtilMethodDigest.digestStaticMethods(
                patched.abc(), patched.classIndex());
        String referenceDisplayDigest = digestDisplayMethod(loadSwf(referencePath));
        String patchedDisplayDigest = digestDisplayMethod(loadSwf(patchedPath));

        requireContains(evidence, "\"className\": \"" + CLASS_NAME + "\"");
        requireContains(evidence, "\"patchMethod\": \"" + RESPONSE_METHOD_NAME + "\"");
        requireContains(evidence, "\"digestVersion\": 2");
        requireContains(evidence, "\"methodOnly\": true");
        requireContains(evidence, "\"changesErrorTextOnly\": true");
        requireContains(evidence, "\"forcesInternalErrorDisplay\": true");
        requireContains(evidence, "\"diagnosticFields\": [\"responseTextLength\",\"responseTextSha256\",\"responseTextPrefix4096Sha256Prefix\",\"responseTextPrefix8192Sha256Prefix\",\"responseTextPrefix12288Sha256Prefix\",\"decodedBytesLength\",\"decodedBytesSha256\",\"decoderPosition\",\"decoderBytesAvailable\"]");
        requireContains(evidence, "\"inputChangesFromReference\": []");
        requireContains(evidence, "\"changedMethods\": [\"requestCompleteHandler\"]");
        requireContains(evidence, "\"referenceSha256\": \"" + referenceDigests.get(RESPONSE_METHOD_NAME) + "\"");
        requireContains(evidence, "\"inputSha256\": \"" + referenceDigests.get(RESPONSE_METHOD_NAME) + "\"");
        requireContains(evidence, "\"outputSha256\": \"" + patchedDigests.get(RESPONSE_METHOD_NAME) + "\"");
        requireContains(evidence, "\"inputSha256\": \"" + referenceDigests.get(REQUEST_METHOD_NAME) + "\"");
        requireContains(evidence, "\"outputSha256\": \"" + referenceDigests.get(REQUEST_METHOD_NAME) + "\"");
        requireContains(evidence, "\"className\": \"" + DISPLAY_CLASS_NAME + "\"");
        requireContains(evidence, "\"patchMethod\": \"" + DISPLAY_METHOD_NAME + "\"");
        requireContains(evidence, "\"referenceSha256\": \"" + referenceDisplayDigest + "\"");
        requireContains(evidence, "\"inputSha256\": \"" + referenceDisplayDigest + "\"");
        requireContains(evidence, "\"outputSha256\": \"" + patchedDisplayDigest + "\"");
    }
    // //// /验证诊断证据字段与方法摘要一致 ////

    // //// 验证重复应用被拒绝 [@x380kkm 2026-08-10] ////
    private static void verifyRepeatedPatchIsRejected(
            Path referencePath,
            Path patchedPath,
            Path repeatedPath) throws Exception {
        Path evidencePath = repeatedPath.resolveSibling("repeated.json");
        try {
            RemoteUtilDecodeDiagnosticPatch.patch(referencePath, patchedPath, repeatedPath, evidencePath);
        } catch (IllegalStateException error) {
            require(error.getMessage() != null
                            && error.getMessage().contains("unexpected changes"),
                    "重复诊断补丁被拒绝, 但错误没有说明输入方法变化: " + error.getMessage());
            return;
        }
        throw new IllegalStateException("解码诊断补丁允许重复应用");
    }
    // //// /验证重复应用被拒绝 ////

    // //// 查找唯一 RemoteUtil 类 [@x380kkm 2026-08-10] ////
    private static RemoteUtilFixture findRemoteUtil(SWF swf) {
        List<RemoteUtilFixture> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(CLASS_NAME);
            if (classIndex >= 0) {
                matches.add(new RemoteUtilFixture(abc, classIndex));
            }
        }
        require(matches.size() == 1, "RemoteUtil 类数量必须为 1: " + matches.size());
        return matches.get(0);
    }
    // //// /查找唯一 RemoteUtil 类 ////

    // //// 读取 SWF [@x380kkm 2026-08-10] ////
    private static SWF loadSwf(Path path) throws Exception {
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF ////

    // //// 计算 DisplayableError 内部错误显示方法摘要 [@x380kkm 2026-08-11] ////
    private static String digestDisplayMethod(SWF swf) throws Exception {
        List<DisplayMethodFixture> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(DISPLAY_CLASS_NAME);
            if (classIndex < 0) {
                continue;
            }
            InstanceInfo instance = abc.instance_info.get(classIndex);
            for (Trait trait : instance.instance_traits.traits) {
                Integer methodInfo = traitMethodInfo(trait);
                if (methodInfo == null) {
                    continue;
                }
                Multiname name = abc.constants.getMultiname(trait.name_index);
                if (name == null || !name.hasOwnName()
                        || !DISPLAY_METHOD_NAME.equals(abc.constants.getString(name.name_index))) {
                    continue;
                }
                MethodBody body = abc.findBody(methodInfo);
                if (body != null) {
                    matches.add(new DisplayMethodFixture(abc, trait, body));
                }
            }
        }
        require(matches.size() == 1, "DisplayableError.getDisplayMessage 数量必须为 1: " + matches.size());
        DisplayMethodFixture match = matches.get(0);
        return RemoteUtilMethodDigest.digestMethod(match.abc(), match.trait(), match.body());
    }
    // //// /计算 DisplayableError 内部错误显示方法摘要 ////

    private static Integer traitMethodInfo(Trait trait) {
        if (trait instanceof TraitMethodGetterSetter method) {
            return method.method_info;
        }
        if (trait instanceof TraitFunction function) {
            return function.method_info;
        }
        return null;
    }

    private static long countReferencedInstructions(
            ABC abc,
            MethodBody body,
            String instructionName,
            String referencedName) {
        return body.getCode().code.stream()
                .filter(instruction -> instruction.definition.instructionName.equals(instructionName))
                .filter(instruction -> instruction.operands != null && instruction.operands.length > 0)
                .filter(instruction -> {
                    Multiname multiname = abc.constants.getMultiname(instruction.operands[0]);
                    return multiname != null
                            && multiname.hasOwnName()
                            && referencedName.equals(abc.constants.getString(multiname.name_index));
                })
                .count();
    }

    private static long countPushedString(ABC abc, MethodBody body, String value) {
        return body.getCode().code.stream()
                .filter(instruction -> instruction.definition.instructionName.equals("pushstring"))
                .filter(instruction -> instruction.operands != null && instruction.operands.length == 1)
                .filter(instruction -> value.equals(abc.constants.getString(instruction.operands[0])))
                .count();
    }

    private static void requireContains(String content, String expected) {
        require(content.contains(expected), "诊断证据缺少字段: " + expected);
    }

    private static void require(boolean condition, String message) {
        if (!condition) {
            throw new IllegalStateException(message);
        }
    }

    private record RemoteUtilFixture(ABC abc, int classIndex) {
    }

    private record DisplayMethodFixture(ABC abc, Trait trait, MethodBody body) {
    }
}
