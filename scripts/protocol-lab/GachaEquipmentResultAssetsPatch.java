// audience: internal
// # gacha-equipment-result-assets-patch
// 装备抽取结果场景的资源组包含当前卡池的概率主表和兑换点数据.
// Java 17 和 FFDec 24 读取 ABC, 资源声明沿用 GachaReleaseCharacterList.
// 方法入口插入独立分支, 原始方法的相对跳转和异常表保持原值.

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

public final class GachaEquipmentResultAssetsPatch {
    private static final String RESOLVER_CLASS = "pinball.asset.AssetGroupResolver";
    private static final String SCENE_CLASS = "pinball.common.data.scene.SceneKind";
    private static final String METHOD_NAME = "getExtraRemainGroup";

    private GachaEquipmentResultAssetsPatch() {
    }

    // //// 为装备结果场景添加当前卡池资源组 [@x380kkm 2026-09-20] ////
    public static void main(String[] arguments) throws Exception {
        if (arguments.length != 4) {
            throw new IllegalArgumentException(
                    "usage: GachaEquipmentResultAssetsPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }
        Path referencePath = Path.of(arguments[0]).toAbsolutePath().normalize();
        Path inputPath = Path.of(arguments[1]).toAbsolutePath().normalize();
        Path outputPath = Path.of(arguments[2]).toAbsolutePath().normalize();
        Path evidencePath = Path.of(arguments[3]).toAbsolutePath().normalize();
        if (outputPath.equals(referencePath) || outputPath.equals(inputPath) || Files.exists(outputPath)) {
            throw new IllegalArgumentException("输出 SWF 路径需要独立且尚未创建");
        }
        Map<String, String> reference = findClass(load(referencePath), RESOLVER_CLASS).digests();
        SWF input = load(inputPath);
        StaticClass resolver = findClass(input, RESOLVER_CLASS);
        Map<String, String> before = resolver.digests();
        AbcMethodDigest.requireSameMethods(reference, before, "资源组解析类与参考输入不一致");
        int sceneIndex = sceneIndex(findClass(input, SCENE_CLASS));
        byte[] prefix = resultAssetGroups(resolver, sceneIndex);
        MethodBody body = resolver.method(METHOD_NAME);
        byte[] original = body.getCodeBytes().clone();
        if (original.length < 2 || original[0] != 0x56 || original[1] != 0
                || body.exceptions.length != 0) {
            throw new IllegalStateException("资源组方法的入口或异常表不符合预期");
        }
        byte[] patched = new byte[prefix.length + original.length];
        System.arraycopy(prefix, 0, patched, 0, prefix.length);
        System.arraycopy(original, 0, patched, prefix.length, original.length);
        body.setCodeBytes(patched);
        body.max_stack = Math.max(body.max_stack, 3);
        ((Tag) resolver.tag()).setModified(true);
        Files.createDirectories(outputPath.getParent());
        Path temporary = Files.createTempFile(outputPath.getParent(), "gacha-result-assets-", ".swf");
        try (OutputStream output = Files.newOutputStream(temporary)) {
            input.saveTo(output);
        }
        StaticClass saved = findClass(load(temporary), RESOLVER_CLASS);
        if (!Arrays.equals(patched, saved.method(METHOD_NAME).getCodeBytes())) {
            throw new IllegalStateException("保存后的资源组方法与写入字节不同");
        }
        Set<String> changes = AbcMethodDigest.changedMethods(before, saved.digests());
        if (!changes.equals(Set.of(METHOD_NAME))) {
            throw new IllegalStateException("资源组解析类包含额外方法变化: " + changes);
        }
        Files.move(temporary, outputPath);
        Files.createDirectories(evidencePath.getParent());
        Files.writeString(evidencePath,
                "{\n  \"className\": \"" + RESOLVER_CLASS + "\",\n"
                        + "  \"changedMethods\": [\"" + METHOD_NAME + "\"],\n"
                        + "  \"scene\": \"GachaEquipmentResult\",\n"
                        + "  \"sceneIndex\": " + sceneIndex + ",\n"
                        + "  \"resourceGroup\": \"GachaReleaseCharacterList([scene.params[0]])\",\n"
                        + "  \"insertedBytes\": " + prefix.length + ",\n"
                        + "  \"originalBodyPreserved\": true\n}\n",
                StandardCharsets.UTF_8);
        System.out.println("装备结果场景加载当前卡池的概率主表和兑换点数据");
    }
    // //// /为装备结果场景添加当前卡池资源组 ////

    // //// 从客户端构造函数读取装备结果场景编号 [@x380kkm 2026-09-20] ////
    private static int sceneIndex(StaticClass scenes) {
        List<AVM2Instruction> code = scenes.method("GachaEquipmentResult").getCode().code;
        List<Integer> values = code.stream()
                .filter(instruction -> instruction.definition.instructionName.equals("pushshort"))
                .map(instruction -> instruction.operands[0]).toList();
        if (values.size() != 1 || values.get(0) < 0 || values.get(0) > 127) {
            throw new IllegalStateException("装备结果场景构造函数的编号不符合预期");
        }
        return values.get(0);
    }

    // //// 复用目标卡池资源组的声明生成入口分支 [@x380kkm 2026-09-20] ////
    private static byte[] resultAssetGroups(StaticClass resolver, int sceneIndex) {
        List<AVM2Instruction> original = resolver.method(METHOD_NAME).getCode().code;
        List<AVM2Instruction> sceneGroups = resolver.method("resolveAssetGroups").getCode().code;
        int index = uniqueOperand(resolver.abc(), original, "getproperty", "index");
        int params = uniqueOperand(resolver.abc(), original, "getproperty", "params");
        int groupClass = uniqueOperand(resolver.abc(), sceneGroups, "getlex", "AssetGroupKind");
        int groupMethod = uniqueOperand(resolver.abc(), sceneGroups, "callproperty", "GachaReleaseCharacterList");
        Set<Integer> arrayAccesses = new java.util.HashSet<>();
        for (AVM2Instruction instruction : original) {
            if (instruction.definition.instructionName.equals("getproperty")
                    && !resolver.abc().constants.getMultiname(instruction.operands[0]).hasOwnName()) {
                arrayAccesses.add(instruction.operands[0]);
            }
        }
        if (arrayAccesses.size() != 1) throw new IllegalStateException("场景参数的数组访问引用不唯一");
        ByteArrayOutputStream selected = new ByteArrayOutputStream();
        instruction(selected, 0x60, groupClass);
        selected.write(0xd2);
        instruction(selected, 0x66, params);
        selected.writeBytes(new byte[] {0x24, 0});
        instruction(selected, 0x66, arrayAccesses.iterator().next());
        selected.write(0x73);
        instruction(selected, 0x56, 1);
        instruction(selected, 0x46, groupMethod);
        selected.write(1);
        instruction(selected, 0x56, 1);
        selected.write(0x48);
        byte[] selectedBytes = selected.toByteArray();
        ByteArrayOutputStream prefix = new ByteArrayOutputStream();
        prefix.write(0xd2);
        instruction(prefix, 0x66, index);
        prefix.writeBytes(new byte[] {0x24, (byte) sceneIndex, 0x14});
        prefix.write(selectedBytes.length);
        prefix.write(0);
        prefix.write(0);
        prefix.writeBytes(selectedBytes);
        return prefix.toByteArray();
    }
    // //// /复用目标卡池资源组的声明生成入口分支 ////

    // //// 唯一匹配属性引用并编码指令 [@x380kkm 2026-09-20] ////
    private static int uniqueOperand(ABC abc, List<AVM2Instruction> code, String opcode, String name) {
        Set<Integer> matches = new java.util.HashSet<>();
        for (AVM2Instruction instruction : code) {
            if (!instruction.definition.instructionName.equals(opcode)) continue;
            Multiname value = abc.constants.getMultiname(instruction.operands[0]);
            if (value != null && value.hasOwnName() && name.equals(abc.constants.getString(value.name_index))) {
                matches.add(instruction.operands[0]);
            }
        }
        if (matches.size() != 1) throw new IllegalStateException("属性引用数量异常: " + name + ": " + matches.size());
        return matches.iterator().next();
    }

    private static void instruction(ByteArrayOutputStream output, int opcode, int value) {
        output.write(opcode);
        do {
            int part = value & 0x7f;
            value >>>= 7;
            output.write(value == 0 ? part : part | 0x80);
        } while (value != 0);
    }
    // //// /唯一匹配属性引用并编码指令 ////

    // //// 读取并唯一定位静态类 [@x380kkm 2026-09-20] ////
    private static StaticClass findClass(SWF swf, String className) {
        List<StaticClass> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int index = abc.findClassByName(className);
            if (index >= 0) matches.add(new StaticClass(tag, abc, index));
        }
        if (matches.size() != 1) throw new IllegalStateException("类数量异常: " + className + ": " + matches.size());
        return matches.get(0);
    }

    private static SWF load(Path path) throws Exception {
        try (InputStream input = Files.newInputStream(path)) {
            return new SWF(input, false);
        }
    }

    private record StaticClass(ABCContainerTag tag, ABC abc, int classIndex) {
        MethodBody method(String name) {
            return AbcMethodDigest.findStaticMethod(abc, classIndex, name);
        }

        Map<String, String> digests() throws Exception {
            return AbcMethodDigest.digestStaticMethods(abc, classIndex);
        }
    }
    // //// /读取并唯一定位静态类 ////
}
