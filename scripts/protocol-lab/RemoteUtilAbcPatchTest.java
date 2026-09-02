// audience: internal
// # remote-util-abc-patch-test
// 此测试使用外部 CN SWF 验证 RemoteUtil 方法签名, 方法体和常量语义变更都会被拒绝.
// 运行前提是 Java 17, Starview 随附的 FFDec 库和未提交的 CN 客户端 SWF.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushFalseIns;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.MethodInfo;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import com.jpexs.decompiler.flash.tags.Tag;

import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;

public final class RemoteUtilAbcPatchTest {
    private static final String CLASS_NAME = "pinball.context.remote.RemoteUtil";
    private static final String RESPONSE_METHOD_NAME = "requestCompleteHandler";
    private static final Set<String> RESULT_CODE_INSTRUCTIONS =
            Set.of("getproperty", "getlex", "findpropstrict", "findproperty");

    private RemoteUtilAbcPatchTest() {
    }

    // //// 运行真实 SWF 回归测试 [@x380kkm 2026-08-03] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: RemoteUtilAbcPatchTest <reference.swf> <temporary-directory>");
        }

        Path referencePath = Path.of(args[0]).toAbsolutePath().normalize();
        Path temporaryDirectory = Path.of(args[1]).toAbsolutePath().normalize();
        Files.createDirectories(temporaryDirectory);

        RemoteUtilAbcPatch.verifyRemoteUtilPreservation(referencePath, referencePath);
        verifyMethodInfoChangeIsRejected(referencePath, temporaryDirectory.resolve("method-info.swf"));
        verifyConstantSemanticChangeIsRejected(referencePath, temporaryDirectory.resolve("constant.swf"));
        verifyMethodBodyChangeIsRejected(referencePath, temporaryDirectory.resolve("method-body.swf"));
        System.out.println("RemoteUtilAbcPatchTest: PASS");
    }
    // //// /运行真实 SWF 回归测试 ////

    // //// 验证方法签名变更被拒绝 [@x380kkm 2026-08-03] ////
    private static void verifyMethodInfoChangeIsRejected(Path referencePath, Path candidatePath) throws Exception {
        SWF candidate = loadSwf(referencePath);
        RemoteUtilFixture remoteUtil = findRemoteUtil(candidate);
        MethodInfo methodInfo = remoteUtil.abc().method_info.get(remoteUtil.responseMethod().method_info);
        methodInfo.flags ^= MethodInfo.FLAG_NEED_ACTIVATION;
        saveSwf(candidate, remoteUtil.tag(), candidatePath);
        requireRejected(referencePath, candidatePath, "method info");
    }
    // //// /验证方法签名变更被拒绝 ////

    // //// 验证常量池语义变更被拒绝 [@x380kkm 2026-08-03] ////
    private static void verifyConstantSemanticChangeIsRejected(Path referencePath, Path candidatePath) throws Exception {
        SWF candidate = loadSwf(referencePath);
        RemoteUtilFixture remoteUtil = findRemoteUtil(candidate);
        Multiname resultCode = findReferencedMultiname(remoteUtil.abc(), remoteUtil.responseMethod(), "result_code");
        String originalName = remoteUtil.abc().constants.getString(resultCode.name_index);
        remoteUtil.abc().constants.setString(resultCode.name_index, originalName + "_tampered");
        saveSwf(candidate, remoteUtil.tag(), candidatePath);
        requireRejected(referencePath, candidatePath, "constant semantics");
    }
    // //// /验证常量池语义变更被拒绝 ////

    // //// 验证未声明方法体变更被拒绝 [@x380kkm 2026-08-03] ////
    private static void verifyMethodBodyChangeIsRejected(Path referencePath, Path candidatePath) throws Exception {
        SWF candidate = loadSwf(referencePath);
        RemoteUtilFixture remoteUtil = findRemoteUtil(candidate);
        remoteUtil.responseMethod().replaceInstruction(
                0,
                new AVM2Instruction(0, new PushFalseIns(), new int[0]));
        remoteUtil.responseMethod().markOffsets();
        remoteUtil.responseMethod().setModified();
        saveSwf(candidate, remoteUtil.tag(), candidatePath);
        requireRejected(referencePath, candidatePath, "method body");
    }
    // //// /验证未声明方法体变更被拒绝 ////

    // //// 要求候选 SWF 被完整性验证拒绝 [@x380kkm 2026-08-03] ////
    private static void requireRejected(Path referencePath, Path candidatePath, String changedPart) throws Exception {
        try {
            RemoteUtilAbcPatch.verifyRemoteUtilPreservation(referencePath, candidatePath);
        } catch (IllegalStateException error) {
            if (error.getMessage() != null
                    && error.getMessage().contains("FFDec import changed RemoteUtil before ABC patch")) {
                return;
            }
            throw error;
        }
        throw new IllegalStateException("RemoteUtil preservation accepted changed " + changedPart);
    }
    // //// /要求候选 SWF 被完整性验证拒绝 ////

    // //// 读取 SWF [@x380kkm 2026-08-03] ////
    private static SWF loadSwf(Path path) throws Exception {
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }
    // //// /读取 SWF ////

    // //// 保存修改后的 SWF [@x380kkm 2026-08-03] ////
    private static void saveSwf(SWF swf, ABCContainerTag tag, Path path) throws Exception {
        ((Tag) tag).setModified(true);
        try (OutputStream output = Files.newOutputStream(path)) {
            swf.saveTo(output);
        }
    }
    // //// /保存修改后的 SWF ////

    // //// 查找唯一 RemoteUtil 和响应方法 [@x380kkm 2026-08-03] ////
    private static RemoteUtilFixture findRemoteUtil(SWF swf) {
        List<RemoteUtilFixture> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int classIndex = abc.findClassByName(CLASS_NAME);
            if (classIndex < 0) {
                continue;
            }
            matches.add(new RemoteUtilFixture(
                    tag,
                    abc,
                    RemoteUtilMethodDigest.findStaticMethod(abc, classIndex, RESPONSE_METHOD_NAME)));
        }
        if (matches.size() != 1) {
            throw new IllegalStateException("RemoteUtil class count must be one: " + matches.size());
        }
        return matches.get(0);
    }
    // //// /查找唯一 RemoteUtil 和响应方法 ////

    // //// 查找方法引用的 multiname [@x380kkm 2026-08-03] ////
    private static Multiname findReferencedMultiname(ABC abc, MethodBody body, String name) {
        for (AVM2Instruction instruction : body.getCode().code) {
            if (!RESULT_CODE_INSTRUCTIONS.contains(instruction.definition.instructionName)
                    || instruction.operands == null
                    || instruction.operands.length == 0) {
                continue;
            }
            Multiname multiname = abc.constants.getMultiname(instruction.operands[0]);
            if (multiname != null
                    && multiname.hasOwnName()
                    && name.equals(abc.constants.getString(multiname.name_index))) {
                return multiname;
            }
        }
        throw new IllegalStateException("RemoteUtil response method is missing multiname: " + name);
    }
    // //// /查找方法引用的 multiname ////

    private record RemoteUtilFixture(ABCContainerTag tag, ABC abc, MethodBody responseMethod) {
    }
}
