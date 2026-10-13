// audience: internal
// # gacha-equipment-exchange-visibility-patch
// 装备抽取结果页按兑换入口的可见性计算兑换点提示.
// Java 17 和 FFDec 24 读取 ABC, 方法入口的条件沿用 didChangeViewScene 的卡池配置.
// 插入条件保留原方法内部的相对跳转, 原始方法的异常表为空.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
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

public final class GachaEquipmentExchangeVisibilityPatch {
    private static final String CLASS_NAME = "pinball.scene.gacha.equipment.result.GachaEquipmentResultScene";
    private static final String METHOD_NAME = "refreshExchangeEquipment";

    private GachaEquipmentExchangeVisibilityPatch() {
    }

    // //// 按兑换入口的可见性保护结果提示计算 [@x380kkm 2026-09-20] ////
    public static void main(String[] arguments) throws Exception {
        if (arguments.length != 4) {
            throw new IllegalArgumentException(
                    "usage: GachaEquipmentExchangeVisibilityPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }
        Path referencePath = Path.of(arguments[0]).toAbsolutePath().normalize();
        Path inputPath = Path.of(arguments[1]).toAbsolutePath().normalize();
        Path outputPath = Path.of(arguments[2]).toAbsolutePath().normalize();
        Path evidencePath = Path.of(arguments[3]).toAbsolutePath().normalize();
        if (outputPath.equals(referencePath) || outputPath.equals(inputPath) || Files.exists(outputPath)) {
            throw new IllegalArgumentException("输出 SWF 路径需要独立且尚未创建");
        }
        Map<String, String> referenceDigests = findScene(load(referencePath)).digests();
        SWF input = load(inputPath);
        ResultScene scene = findScene(input);
        Map<String, String> before = scene.digests();
        AbcMethodDigest.requireSameMethods(referenceDigests, before, "装备结果类与参考输入不一致");
        byte[] guard = visibilityGuard(scene);
        MethodBody body = scene.method(METHOD_NAME);
        byte[] original = body.getCodeBytes().clone();
        if (original.length < 2 || original[0] != (byte) 0xd0 || original[1] != 0x30
                || body.exceptions.length != 0 || body.max_stack < 1) {
            throw new IllegalStateException("结果提示方法的入口或异常表不符合预期");
        }
        byte[] patched = new byte[original.length + guard.length];
        System.arraycopy(original, 0, patched, 0, 2);
        System.arraycopy(guard, 0, patched, 2, guard.length);
        System.arraycopy(original, 2, patched, 2 + guard.length, original.length - 2);
        body.setCodeBytes(patched);
        ((Tag) scene.tag()).setModified(true);
        Files.createDirectories(outputPath.getParent());
        Path temporary = Files.createTempFile(outputPath.getParent(), "gacha-exchange-", ".swf");
        try (OutputStream output = Files.newOutputStream(temporary)) {
            input.saveTo(output);
        }
        ResultScene saved = findScene(load(temporary));
        if (!Arrays.equals(patched, saved.method(METHOD_NAME).getCodeBytes())) {
            throw new IllegalStateException("保存后的结果提示方法与写入字节不同");
        }
        Set<String> changes = AbcMethodDigest.changedMethods(before, saved.digests());
        if (!changes.equals(Set.of(METHOD_NAME))) {
            throw new IllegalStateException("装备结果类包含额外方法变化: " + changes);
        }
        Files.move(temporary, outputPath);
        Files.createDirectories(evidencePath.getParent());
        Files.writeString(evidencePath,
                "{\n  \"className\": \"" + CLASS_NAME + "\",\n"
                        + "  \"changedMethods\": [\"" + METHOD_NAME + "\"],\n"
                        + "  \"condition\": \"gachaLogic.isFreemiumGuaranteeAvailable()\",\n"
                        + "  \"visibilityMethod\": \"didChangeViewScene\",\n"
                        + "  \"insertedBytes\": " + guard.length + ",\n"
                        + "  \"originalBodyPreserved\": true\n}\n",
                StandardCharsets.UTF_8);
        System.out.println("装备结果提示采用兑换入口的可见性条件");
    }
    // //// /按兑换入口的可见性保护结果提示计算 ////

    // //// 从兑换入口读取字段和条件的方法引用 [@x380kkm 2026-09-20] ////
    private static byte[] visibilityGuard(ResultScene scene) {
        List<AVM2Instruction> code = scene.method("didChangeViewScene").getCode().code;
        int property = uniqueOperand(scene.abc(), code, "getproperty", "gachaLogic");
        int predicate = uniqueOperand(scene.abc(), code, "callproperty", "isFreemiumGuaranteeAvailable");
        uniqueOperand(scene.abc(), code, "callpropvoid", "showExchangeEquipment");
        List<AVM2Instruction> target = scene.method(METHOD_NAME).getCode().code;
        if (target.stream().anyMatch(instruction -> hasName(scene.abc(), instruction, "isFreemiumGuaranteeAvailable"))) {
            throw new IllegalStateException("结果提示方法已包含兑换可见性条件");
        }
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        bytes.write(0xd0);
        bytes.write(0x66);
        writeU30(bytes, property);
        bytes.write(0x46);
        writeU30(bytes, predicate);
        bytes.write(0);
        bytes.writeBytes(new byte[] {0x11, 1, 0, 0, 0x47});
        return bytes.toByteArray();
    }
    // //// /从兑换入口读取字段和条件的方法引用 ////

    // //// 唯一匹配方法体中的属性引用 [@x380kkm 2026-09-20] ////
    private static int uniqueOperand(ABC abc, List<AVM2Instruction> code, String opcode, String name) {
        Set<Integer> matches = new java.util.HashSet<>();
        for (AVM2Instruction instruction : code) {
            if (instruction.definition.instructionName.equals(opcode) && hasName(abc, instruction, name)) {
                if (opcode.startsWith("call") && instruction.operands[1] != 0) {
                    throw new IllegalStateException("兑换可见性方法的参数数量异常: " + name);
                }
                matches.add(instruction.operands[0]);
            }
        }
        if (matches.size() != 1) throw new IllegalStateException("属性引用数量异常: " + name + ": " + matches.size());
        return matches.iterator().next();
    }

    private static boolean hasName(ABC abc, AVM2Instruction instruction, String name) {
        if (instruction.operands == null || instruction.operands.length == 0) return false;
        if (!Set.of("getproperty", "callproperty", "callpropvoid").contains(instruction.definition.instructionName)) return false;
        Multiname value = abc.constants.getMultiname(instruction.operands[0]);
        return value != null && value.hasOwnName() && name.equals(abc.constants.getString(value.name_index));
    }

    private static void writeU30(ByteArrayOutputStream output, int value) {
        do {
            int part = value & 0x7f;
            value >>>= 7;
            output.write(value == 0 ? part : part | 0x80);
        } while (value != 0);
    }
    // //// /唯一匹配方法体中的属性引用 ////

    // //// 读取并唯一定位装备结果类 [@x380kkm 2026-09-20] ////
    private static ResultScene findScene(SWF swf) {
        List<ResultScene> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int index = abc.findClassByName(CLASS_NAME);
            if (index >= 0) matches.add(new ResultScene(tag, abc, index));
        }
        if (matches.size() != 1) throw new IllegalStateException("装备结果类数量异常: " + matches.size());
        return matches.get(0);
    }

    private static SWF load(Path path) throws Exception {
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }

    private record ResultScene(ABCContainerTag tag, ABC abc, int classIndex) {
        MethodBody method(String name) {
            return AbcMethodDigest.findInstanceMethod(abc, classIndex, name);
        }

        Map<String, String> digests() throws Exception {
            return AbcMethodDigest.digestInstanceMethods(abc, classIndex);
        }
    }
    // //// /读取并唯一定位装备结果类 ////
}
