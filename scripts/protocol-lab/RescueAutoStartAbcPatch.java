// audience: internal
// # rescue-auto-start-abc-patch
// 救援访客选择房间时开启本次续战, 恢复连接时保留传入的房间状态.
// Java 17 和 FFDec 24 读取方法体. 插入指令复用原续战寄存器, 房间按钮和关卡设置由原流程处理.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.InstructionDefinition;
import com.jpexs.decompiler.flash.abc.avm2.instructions.jumps.IfNeIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.jumps.IfTrueIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.jumps.JumpIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.localregs.GetLocalIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.localregs.SetLocalIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.GetPropertyIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushByteIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushTrueIns;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import com.jpexs.decompiler.flash.tags.Tag;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class RescueAutoStartAbcPatch {
    private static final String CLASS_NAME = "pinball.loading.multi.EnterRoomService";
    private static final int AUTO_START_LOCAL = 2;

    private RescueAutoStartAbcPatch() { }

    // //// 写入救援默认续战补丁并检查方法范围 [@x380kkm 2026-09-23] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException("usage: RescueAutoStartAbcPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }
        Path outputPath = Path.of(args[2]).toAbsolutePath().normalize();
        if (Files.exists(outputPath) || outputPath.equals(Path.of(args[0]).toAbsolutePath().normalize())
                || outputPath.equals(Path.of(args[1]).toAbsolutePath().normalize())) {
            throw new IllegalArgumentException("输出 SWF 使用独立的新文件路径");
        }
        SWF reference = load(Path.of(args[0]));
        SWF input = load(Path.of(args[1]));
        ClassView target = find(input);
        Map<String, String> before = target.digests();
        AbcMethodDigest.requireSameMethods(find(reference).digests(), before, "入房方法与参考输入不同");
        byte[] expected = patch(target);
        verify(input, before, expected);
        Files.createDirectories(outputPath.getParent());
        Path temporary = Files.createTempFile(outputPath.getParent(), "rescue-auto-start-", ".swf");
        try (OutputStream stream = Files.newOutputStream(temporary)) { input.saveTo(stream); }
        verify(load(temporary), before, expected);
        Files.move(temporary, outputPath);
        Path evidencePath = Path.of(args[3]);
        Files.createDirectories(evidencePath.toAbsolutePath().getParent());
        Files.writeString(evidencePath, "{\"scope\":\"attention-guest\",\"selectAutoStart\":true,"
                + "\"resumeMode\":\"continuation\",\"questPreferencesPreserved\":true,\"entryBodyPreserved\":true,"
                + "\"changedMethods\":[\"" + CLASS_NAME + ".run\"]}\n", StandardCharsets.UTF_8);
        System.out.println("救援访客选择房间时默认续战, 恢复连接保留本次模式, 原关卡设置保持不变");
    }
    // //// /写入救援默认续战补丁并检查方法范围 ////

    // //// 在读取关卡设置后选择救援房间续战状态 [@x380kkm 2026-09-23] ////
    private static byte[] patch(ClassView owner) {
        MethodBody body = owner.run();
        List<AVM2Instruction> original = body.getCode().code;
        if (body.exceptions.length != 0 || body.max_regs != AUTO_START_LOCAL + 1
                || original.stream().anyMatch(instruction -> instruction.isBranch() || instruction.isJump())) {
            throw new IllegalStateException("入房方法的寄存器或分支结构不符合客户端定义");
        }
        int readIndex = namedInstruction(owner.abc(), body, "callproperty", "getAutoStartBattle");
        requireOpcodes(body, readIndex, "callproperty", "convert_b", "convert_b", "setlocal2",
                "findproperty", "getproperty", "getlocal2", "initproperty");
        int insertionIndex = readIndex + 4;
        List<AVM2Instruction> additions = rescueContinuation(owner.abc(), original.get(readIndex + 1));
        int offset = Math.toIntExact(original.get(insertionIndex).getAddress());
        byte[] source = body.getCodeBytes();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        output.write(source, 0, offset);
        for (AVM2Instruction instruction : additions) { output.writeBytes(instruction.getBytes()); }
        output.write(source, offset, source.length - offset);
        byte[] result = output.toByteArray();
        body.setCodeBytes(result);
        body.getCode().checkValidOffsets(body);
        ((Tag) owner.tag()).setModified(true);
        return result;
    }
    // //// /在读取关卡设置后选择救援房间续战状态 ////

    // //// 设置救援访客的本次续战模式 [@x380kkm 2026-09-23] ////
    private static List<AVM2Instruction> rescueContinuation(ABC abc, AVM2Instruction convertBoolean) {
        List<AVM2Instruction> code = new ArrayList<>();
        List<AVM2Instruction> skip = new ArrayList<>();
        localProperties(code, abc, 0, "config", "attentionKey", "index");
        append(code, new PushByteIns(), 0);
        skip.add(append(code, new IfNeIns(), 0));
        localProperties(code, abc, 0, "continuationData", "isHost");
        skip.add(append(code, new IfTrueIns(), 0));
        localProperties(code, abc, 0, "reason", "index");
        append(code, new PushByteIns(), 0);
        AVM2Instruction resume = append(code, new IfNeIns(), 0);
        append(code, new PushTrueIns());
        append(code, new SetLocalIns(), AUTO_START_LOCAL);
        skip.add(append(code, new JumpIns(), 0));
        int restore = code.size();
        localProperties(code, abc, 0, "continuationData", "isAutoStartMode");
        code.add(convertBoolean.clone());
        append(code, new SetLocalIns(), AUTO_START_LOCAL);
        target(code, resume, restore);
        for (AVM2Instruction branch : skip) { target(code, branch, code.size()); }
        return code;
    }
    // //// /设置救援访客的本次续战模式 ////

    // //// 组装带相对跳转的插入指令 [@x380kkm 2026-09-23] ////
    private static AVM2Instruction append(List<AVM2Instruction> code, InstructionDefinition definition, int... operands) {
        AVM2Instruction instruction = new AVM2Instruction(0, definition, operands);
        code.add(instruction);
        return instruction;
    }

    private static void localProperties(List<AVM2Instruction> code, ABC abc, int local, String... names) {
        append(code, new GetLocalIns(), local);
        for (String name : names) { append(code, new GetPropertyIns(), publicName(abc, name)); }
    }

    private static void target(List<AVM2Instruction> code, AVM2Instruction branch, int targetIndex) {
        int branchEnd = 0;
        int destination = 0;
        int position = 0;
        for (int index = 0; index < code.size(); index++) {
            if (code.get(index) == branch) { branchEnd = position + branch.getBytesLength(); }
            if (index < targetIndex) { destination += code.get(index).getBytesLength(); }
            position += code.get(index).getBytesLength();
        }
        branch.operands[0] = destination - branchEnd;
    }

    private static int publicName(ABC abc, String name) { return abc.constants.getPublicQnameId(name, true); }

    // //// /组装带相对跳转的插入指令 ////

    // //// 定位续战设置读取指令并检查原指令序列 [@x380kkm 2026-09-23] ////
    private static int namedInstruction(ABC abc, MethodBody body, String opcode, String name) {
        List<Integer> matches = new ArrayList<>();
        List<AVM2Instruction> code = body.getCode().code;
        for (int index = 0; index < code.size(); index++) {
            AVM2Instruction instruction = code.get(index);
            if (!instruction.definition.instructionName.equals(opcode)) { continue; }
            Multiname value = abc.constants.getMultiname(instruction.operands[0]);
            if (value != null && value.hasOwnName() && name.equals(abc.constants.getString(value.name_index))) { matches.add(index); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("入房指令引用不唯一: " + name); }
        return matches.get(0);
    }

    private static void requireOpcodes(MethodBody body, int start, String... expected) {
        List<AVM2Instruction> code = body.getCode().code;
        if (start < 0 || start + expected.length > code.size()) { throw new IllegalStateException("入房指令区间越界"); }
        for (int index = 0; index < expected.length; index++) {
            if (!code.get(start + index).definition.instructionName.equals(expected[index])) {
                throw new IllegalStateException("入房指令不符合预期: " + expected[index]);
            }
        }
    }
    // //// /定位续战设置读取指令并检查原指令序列 ////

    // //// 读取目标类并限制补丁的方法范围 [@x380kkm 2026-09-23] ////
    private static void verify(SWF swf, Map<String, String> before, byte[] expected) throws Exception {
        ClassView owner = find(swf);
        if (!AbcMethodDigest.changedMethods(before, owner.digests()).equals(Set.of("run"))
                || !Arrays.equals(expected, owner.run().getCodeBytes()) || owner.run().max_regs != AUTO_START_LOCAL + 1) {
            throw new IllegalStateException("救援续战补丁的方法范围或指令内容不符合预期");
        }
        owner.run().getCode().checkValidOffsets(owner.run());
    }

    private static SWF load(Path path) throws Exception {
        try (InputStream stream = Files.newInputStream(path)) { return new SWF(stream, false); }
    }

    private static ClassView find(SWF swf) {
        List<ClassView> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            int index = tag.getABC().findClassByName(CLASS_NAME);
            if (index >= 0) { matches.add(new ClassView(tag, tag.getABC(), index)); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("入房类数量异常: " + matches.size()); }
        return matches.get(0);
    }

    private record ClassView(ABCContainerTag tag, ABC abc, int index) {
        MethodBody run() { return AbcMethodDigest.findInstanceMethod(abc, index, "run"); }
        Map<String, String> digests() throws Exception { return AbcMethodDigest.digestInstanceMethods(abc, index); }
    }
    // //// /读取目标类并限制补丁的方法范围 ////
}
