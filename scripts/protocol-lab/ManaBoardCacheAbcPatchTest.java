// audience: internal
// # mana-board-cache-abc-patch-test
// 此程序解释真实 SWF 中两个缓存 getter 的指令, 检查二板时间边界和稳定范围内的对象复用.
// 角色 1 的 23 个一板节点和 18 个二板节点提供能力槽输入, 主表加载与界面绘制由调用模型表示.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;

import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.function.Supplier;
import java.util.stream.IntStream;

public final class ManaBoardCacheAbcPatchTest {
    private static final Object NONE = new Object();
    private static final Object OPTION = new Object();

    private ManaBoardCacheAbcPatchTest() { }

    // //// 在真实方法指令上复现二板开放前后的能力读取 [@x380kkm 2026-09-24] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException("usage: ManaBoardCacheAbcPatchTest <original.swf> <patched.swf>");
        }
        Runtime original = load(Path.of(args[0]));
        Runtime patched = load(Path.of(args[1]));
        for (Runtime runtime : List.of(original, patched)) {
            runtime.open = false;
            require(runtime.abilities().equals(List.of(1, 2, 3)), "一板能力范围错误");
        }
        original.open = true;
        patched.open = true;
        require(original.abilities().equals(List.of(1, 2, 3)), "参考输入未复现旧能力缓存");
        require(patched.abilities().equals(List.of(1, 2, 3, 4, 5, 6)), "开放二板后能力缓存未刷新");
        Object abilities = patched.abilities();
        Object nodes = patched.run("getMaxManaNodes");
        require(abilities == patched.abilities() && nodes == patched.run("getMaxManaNodes"), "稳定范围未复用缓存");
        patched.open = false;
        require(((List<?>) patched.run("getMaxManaNodes")).size() == 23, "回拨至一板时节点缓存未刷新");
        require(patched.abilities().equals(List.of(1, 2, 3)), "回拨至一板时能力缓存未刷新");

        Runtime normal = load(Path.of(args[1]));
        normal.open = true;
        require(normal.abilities().equals(List.of(1, 2, 3, 4, 5, 6)), "正常二板能力范围错误");
        System.out.println("参考输入跨开放时间后保留 3 个能力槽, 补丁输出刷新至 6 个; 正常二板, 时间回拨和稳定缓存复用符合预期");
    }
    // //// /在真实方法指令上复现二板开放前后的能力读取 ////

    // //// 解释缓存 getter 使用的指令与延迟对象调用 [@x380kkm 2026-09-24] ////
    private static final class Runtime {
        final ABC abc;
        final int classIndex;
        boolean open;
        final Lazy nodes = new Lazy(this::visibleNodeIds);
        final Lazy abilities = new Lazy(() -> {
            List<?> ids = (List<?>) run("getMaxManaNodes");
            return ids.size() == 41 ? List.of(1, 2, 3, 4, 5, 6) : List.of(1, 2, 3);
        });

        Runtime(ABC abc, int classIndex) { this.abc = abc; this.classIndex = classIndex; }

        List<Integer> visibleNodeIds() {
            return IntStream.concat(IntStream.rangeClosed(2201, 2223), open ? IntStream.rangeClosed(2401, 2418) : IntStream.empty())
                    .boxed().toList();
        }

        List<?> abilities() { return (List<?>) run("getMaxAbilities"); }

        Object run(String method) {
            MethodBody body = AbcMethodDigest.findInstanceMethod(abc, classIndex, method);
            List<AVM2Instruction> code = body.getCode().code;
            Object[] locals = new Object[body.max_regs];
            locals[0] = this;
            List<Object> stack = new ArrayList<>();
            for (int index = 0; index < code.size(); index++) {
                AVM2Instruction instruction = code.get(index);
                switch (instruction.definition.instructionName) {
                    case "getlocal0" -> stack.add(locals[0]);
                    case "getlocal1" -> stack.add(locals[1]);
                    case "getlocal" -> stack.add(locals[instruction.operands[0]]);
                    case "findproperty" -> {
                        require(name(instruction).equals("_maxManaNodes") || name(instruction).equals("_maxAbilities"), "缓存 getter 查找未解释属性");
                        stack.add(this);
                    }
                    case "setlocal" -> locals[instruction.operands[0]] = pop(stack);
                    case "setlocal1" -> locals[1] = pop(stack);
                    case "pushscope", "pop" -> pop(stack);
                    case "coerce" -> { }
                    case "getlex" -> {
                        require(name(instruction).equals("Option"), "类常量超出缓存 getter 范围");
                        stack.add(OPTION);
                    }
                    case "getproperty" -> stack.add(property(pop(stack), name(instruction)));
                    case "setproperty" -> {
                        Object value = pop(stack);
                        Lazy owner = (Lazy) pop(stack);
                        require(name(instruction).equals("value") && value == NONE, "缓存写入超出定义");
                        owner.value = NONE;
                    }
                    case "callproperty" -> {
                        require(instruction.operands[1] == 0, "缓存 getter 调用带有参数");
                        Object owner = pop(stack);
                        String called = name(instruction);
                        stack.add(owner instanceof Lazy lazy ? lazy.get()
                                : called.equals("getManaNodeIds") ? visibleNodeIds() : run(called));
                    }
                    case "ifeq" -> {
                        Object right = pop(stack);
                        Object left = pop(stack);
                        if (left.equals(right)) {
                            long target = instruction.getAddress() + instruction.getBytesLength() + instruction.operands[0];
                            int destination = -1;
                            for (int candidate = 0; candidate < code.size(); candidate++) {
                                if (code.get(candidate).getAddress() == target) { destination = candidate; break; }
                            }
                            require(destination >= 0, "跳转目标不存在");
                            index = destination - 1;
                        }
                    }
                    case "returnvalue" -> { return pop(stack); }
                    default -> throw new IllegalStateException("缓存 getter 存在未解释指令: " + instruction.definition.instructionName);
                }
            }
            throw new IllegalStateException("缓存 getter 未返回");
        }

        String name(AVM2Instruction instruction) {
            return abc.constants.getString(abc.constants.getMultiname(instruction.operands[0]).name_index);
        }

        Object property(Object owner, String name) {
            if (owner == OPTION && name.equals("None")) { return NONE; }
            if (owner instanceof List<?> list && name.equals("length")) { return list.size(); }
            if (owner == this && name.equals("_maxManaNodes")) { return nodes; }
            if (owner == this && name.equals("_maxAbilities")) { return abilities; }
            throw new IllegalStateException("缓存 getter 访问未解释属性: " + name);
        }
    }
    // //// /解释缓存 getter 使用的指令与延迟对象调用 ////

    // //// 提供延迟缓存与真实方法体输入 [@x380kkm 2026-09-24] ////
    private static final class Lazy {
        final Supplier<Object> getter;
        Object value = NONE;
        Lazy(Supplier<Object> getter) { this.getter = getter; }
        Object get() {
            if (value == NONE) { value = getter.get(); }
            return value;
        }
    }

    private static Object pop(List<Object> stack) { return stack.remove(stack.size() - 1); }

    private static void require(boolean condition, String message) {
        if (!condition) { throw new IllegalStateException(message); }
    }

    private static Runtime load(Path path) throws Exception {
        SWF swf;
        try (InputStream stream = Files.newInputStream(path)) { swf = new SWF(stream, false); }
        for (ABCContainerTag tag : swf.getAbcList()) {
            int index = tag.getABC().findClassByName("pinball.common.data.character.GeneralCharacterLogic");
            if (index >= 0) { return new Runtime(tag.getABC(), index); }
        }
        throw new IllegalStateException("输入缺少角色逻辑类");
    }
    // //// /提供延迟缓存与真实方法体输入 ////
}
