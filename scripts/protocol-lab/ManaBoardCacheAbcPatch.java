// audience: internal
// # mana-board-cache-abc-patch
// 玛纳节点和最大能力使用相同的可见板范围. 板范围改变时同时刷新两份延迟缓存.
// Java 17 和 FFDec 24 读取 Android 1.8.1 方法体, 输出路径保存独立的 SWF 副本.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.InstructionDefinition;
import com.jpexs.decompiler.flash.abc.avm2.instructions.executing.CallPropertyIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.jumps.IfEqIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.localregs.GetLocalIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.localregs.SetLocalIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.GetLexIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.GetPropertyIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.ReturnValueIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.other.SetPropertyIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PopIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.stack.PushScopeIns;
import com.jpexs.decompiler.flash.abc.avm2.instructions.types.CoerceIns;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Namespace;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import com.jpexs.decompiler.flash.tags.Tag;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class ManaBoardCacheAbcPatch {
    private static final String CLASS_NAME = "pinball.common.data.character.GeneralCharacterLogic";
    private static final Set<String> METHODS = Set.of("getMaxManaNodes", "getMaxAbilities");

    private ManaBoardCacheAbcPatch() { }

    // //// 保存玛纳板缓存补丁并检查方法边界 [@x380kkm 2026-09-24] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException("usage: ManaBoardCacheAbcPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }
        Path outputPath = Path.of(args[2]).toAbsolutePath().normalize();
        if (Files.exists(outputPath) || outputPath.equals(Path.of(args[0]).toAbsolutePath().normalize())
                || outputPath.equals(Path.of(args[1]).toAbsolutePath().normalize())) {
            throw new IllegalArgumentException("输出 SWF 使用独立的新文件路径");
        }
        SWF input = load(Path.of(args[1]));
        ClassView owner = find(input);
        Map<String, String> before = owner.digests();
        AbcMethodDigest.requireSameMethods(find(load(Path.of(args[0]))).digests(), before, "角色方法与参考输入不同");
        patch(owner);
        verify(owner, before);
        Files.createDirectories(outputPath.getParent());
        try (OutputStream stream = Files.newOutputStream(outputPath)) { input.saveTo(stream); }
        verify(find(load(outputPath)), before);
        Path evidencePath = Path.of(args[3]);
        Files.createDirectories(evidencePath.toAbsolutePath().getParent());
        Files.writeString(evidencePath, "{\"class\":\"" + CLASS_NAME + "\","
                + "\"cacheScope\":\"visible-mana-nodes\",\"refreshesAbilityCache\":true,"
                + "\"changedMethods\":[\"getMaxManaNodes\",\"getMaxAbilities\"]}\n", StandardCharsets.UTF_8);
        System.out.println("玛纳板可见范围改变时刷新节点与最大能力缓存");
    }
    // //// /保存玛纳板缓存补丁并检查方法边界 ////

    // //// 在读取最大能力前同步玛纳节点缓存 [@x380kkm 2026-09-24] ////
    private static void patch(ClassView owner) {
        MethodBody nodes = owner.method("getMaxManaNodes");
        MethodBody abilities = owner.method("getMaxAbilities");
        requireCachedGetter(owner.abc(), nodes, "_maxManaNodes");
        requireCachedGetter(owner.abc(), abilities, "_maxAbilities");
        ABC abc = owner.abc();
        List<AVM2Instruction> code = new ArrayList<>();
        scope(code);
        cachedValue(code, abc, "_maxManaNodes");
        append(code, new SetLocalIns(), 1);
        append(code, new GetLocalIns(), 1);
        append(code, new GetPropertyIns(), publicName(abc, "length"));
        append(code, new GetLocalIns(), 0);
        append(code, new CallPropertyIns(), publicName(abc, "getManaNodeIds"), 0);
        append(code, new GetPropertyIns(), publicName(abc, "length"));
        AVM2Instruction equal = append(code, new IfEqIns(), 0);
        for (String field : List.of("_maxManaNodes", "_maxAbilities")) {
            append(code, new GetLocalIns(), 0);
            append(code, new GetPropertyIns(), publicName(abc, field));
            append(code, new GetLexIns(), abc.constants.getQnameId("Option", Namespace.KIND_PACKAGE, "haxe.ds", true));
            append(code, new GetPropertyIns(), publicName(abc, "None"));
            append(code, new SetPropertyIns(), publicName(abc, "value"));
        }
        cachedValue(code, abc, "_maxManaNodes");
        append(code, new SetLocalIns(), 1);
        branchToEnd(code, equal);
        append(code, new GetLocalIns(), 1);
        append(code, new ReturnValueIns());
        write(nodes, code, 2);

        code = new ArrayList<>();
        scope(code);
        append(code, new GetLocalIns(), 0);
        append(code, new CallPropertyIns(), publicName(abc, "getMaxManaNodes"), 0);
        append(code, new PopIns());
        cachedValue(code, abc, "_maxAbilities");
        append(code, new ReturnValueIns());
        write(abilities, code, 1);
        ((Tag) owner.tag()).setModified(true);
    }
    // //// /在读取最大能力前同步玛纳节点缓存 ////

    // //// 组装节点范围检查和延迟缓存读取指令 [@x380kkm 2026-09-24] ////
    private static void scope(List<AVM2Instruction> code) {
        append(code, new GetLocalIns(), 0);
        append(code, new PushScopeIns());
    }

    private static void cachedValue(List<AVM2Instruction> code, ABC abc, String field) {
        append(code, new GetLocalIns(), 0);
        append(code, new GetPropertyIns(), publicName(abc, field));
        append(code, new CallPropertyIns(), publicName(abc, "get"), 0);
        append(code, new CoerceIns(), publicName(abc, "Array"));
    }

    private static AVM2Instruction append(List<AVM2Instruction> code, InstructionDefinition definition, int... operands) {
        AVM2Instruction instruction = new AVM2Instruction(0, definition, operands);
        code.add(instruction);
        return instruction;
    }

    private static void branchToEnd(List<AVM2Instruction> code, AVM2Instruction branch) {
        int end = 0;
        int branchEnd = 0;
        for (AVM2Instruction instruction : code) {
            end += instruction.getBytesLength();
            if (instruction == branch) { branchEnd = end; }
        }
        branch.operands[0] = end - branchEnd;
    }

    private static void write(MethodBody body, List<AVM2Instruction> code, int registers) {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        for (AVM2Instruction instruction : code) { bytes.writeBytes(instruction.getBytes()); }
        body.setCodeBytes(bytes.toByteArray());
        body.max_regs = registers;
        body.max_stack = 2;
        body.getCode().checkValidOffsets(body);
    }

    private static int publicName(ABC abc, String name) { return abc.constants.getPublicQnameId(name, true); }
    // //// /组装节点范围检查和延迟缓存读取指令 ////

    // //// 检查客户端缓存读取方法的原始形状 [@x380kkm 2026-09-24] ////
    private static void requireCachedGetter(ABC abc, MethodBody body, String field) {
        List<AVM2Instruction> code = body.getCode().code;
        if (body.exceptions.length != 0 || code.stream().anyMatch(item -> item.isBranch() || item.isJump())) {
            throw new IllegalStateException("缓存读取方法存在额外控制流");
        }
        long references = code.stream().filter(item -> item.definition.instructionName.equals("getproperty"))
                .filter(item -> field.equals(abc.constants.getString(abc.constants.getMultiname(item.operands[0]).name_index))).count();
        if (references != 1 || code.size() != 7) {
            throw new IllegalStateException("缓存读取方法不符合客户端定义: " + field + ", 指令数 " + code.size());
        }
    }

    private static void verify(ClassView owner, Map<String, String> before) throws Exception {
        if (!AbcMethodDigest.changedMethods(before, owner.digests()).equals(METHODS)) {
            throw new IllegalStateException("玛纳缓存补丁的方法范围不符合定义");
        }
        for (String name : METHODS) { owner.method(name).getCode().checkValidOffsets(owner.method(name)); }
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
        if (matches.size() != 1) { throw new IllegalStateException("角色类数量异常: " + matches.size()); }
        return matches.get(0);
    }

    private record ClassView(ABCContainerTag tag, ABC abc, int index) {
        MethodBody method(String name) { return AbcMethodDigest.findInstanceMethod(abc, index, name); }
        Map<String, String> digests() throws Exception { return AbcMethodDigest.digestInstanceMethods(abc, index); }
    }
    // //// /检查客户端缓存读取方法的原始形状 ////
}
