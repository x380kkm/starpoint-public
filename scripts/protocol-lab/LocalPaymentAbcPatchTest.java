// audience: internal
// # local-payment-abc-patch-test
// 此入口使用真实客户端核对付款闭包引用, 并回放商店刷新方法的 AVM2 分支.

import com.jpexs.decompiler.flash.abc.types.MethodBody;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class LocalPaymentAbcPatchTest {
    private LocalPaymentAbcPatchTest() { }

    // //// 核对闭包重编号并拒绝内容不同的回调 [@x380kkm 2026-09-26] ////
    public static void main(String[] args) throws Exception {
        var reference = LocalPaymentAbcPatch.load(Path.of(args[0]));
        var input = LocalPaymentAbcPatch.load(Path.of(args[1]));
        var expected = LocalPaymentAbcPatch.find(reference, LocalPaymentAbcPatch.PLATFORM);
        var actual = LocalPaymentAbcPatch.find(input, LocalPaymentAbcPatch.PLATFORM);
        var before = actual.digests();
        for (String name : LocalPaymentAbcPatch.METHODS.keySet()) {
            var original = LocalPaymentAbcPatch.find(reference, name);
            var current = LocalPaymentAbcPatch.find(input, name);
            AbcMethodDigest.requireSameInstanceMethods(original.abc(), original.index(),
                    current.abc(), current.index(), "付款参考方法不同");
        }
        int closureIndex = actual.method("waitQuestPurchargeResult").getCode().code.stream()
                .filter(instruction -> instruction.definition.instructionName.equals("newfunction"))
                .findFirst().orElseThrow().operands[0];
        MethodBody closure = actual.abc().findBody(closureIndex);
        byte[] originalCode = closure.getCodeBytes().clone();
        boolean rejected = false;
        try {
            closure.setCodeBytes(new byte[] {0x47});
            AbcMethodDigest.requireSameInstanceMethods(expected.abc(), expected.index(),
                    actual.abc(), actual.index(), "付款参考方法不同");
        } catch (IllegalStateException exception) {
            if (!exception.getMessage().contains("waitQuestPurchargeResult: 闭包")) { throw exception; }
            rejected = true;
        } finally {
            closure.setCodeBytes(originalCode);
        }
        if (!rejected) { throw new IllegalStateException("内容不同的付款闭包未被拒绝"); }
        AbcMethodDigest.requireSameMethods(before, actual.digests(), "只读核对改变了付款方法");
        var shopView = LocalPaymentAbcPatch.find(input, LocalPaymentAbcPatch.SHOP_VIEW);
        LocalPaymentAbcPatch.synchronizeStoneBuyList(shopView);
        verifyTabChanges(shopView);
        System.out.println("付款闭包重编号保留实际方法核对, 内容不同的闭包被拒绝");
    }
    // //// /核对闭包重编号并拒绝内容不同的回调 ////

    // //// 回放月卡页和索引四横幅页之间的切换 [@x380kkm 2026-09-28] ////
    private static void verifyTabChanges(LocalPaymentAbcPatch.ClassView owner) {
        StoreScene scene = new StoreScene();
        scene.products = List.of(0, 0);
        executeRefresh(owner, scene);
        scene.products = List.of(0, 0, 0, 0, 1, 0);
        executeRefresh(owner, scene);
        Button original = scene.buttons.get(4);
        original.enabled = false;
        executeRefresh(owner, scene);
        require(scene.buttons.get(4) == original && !original.enabled, "重复刷新改变了已有按钮状态");
        scene.products = List.of(1, 0, 0, 0, 0, 0, 0);
        executeRefresh(owner, scene);
        scene.products = List.of();
        executeRefresh(owner, scene);
        scene.products = List.of(1, 0, 0, 0, 1, 0);
        executeRefresh(owner, scene);
        scene.products = List.of(0, 0);
        executeRefresh(owner, scene);
        require(scene.buttons.isEmpty() && scene.renders == 7, "页签刷新次数或最终按钮集合异常");
        System.out.println("真实商店刷新字节码回放七次切页, 横幅索引, 回调归属和已有状态保持一致");
    }

    private static void require(boolean condition, String message) {
        if (!condition) { throw new IllegalStateException(message); }
    }

    private static final class Button {
        final StoreScene callbackOwner;
        boolean enabled = true;
        Button(StoreScene callbackOwner) { this.callbackOwner = callbackOwner; }
    }

    private static final class StoreScene {
        List<Integer> products = List.of();
        final List<Integer> kinds = new ArrayList<>();
        final Map<Integer, Button> buttons = new LinkedHashMap<>();
        int renders;

        Object call(String name, List<Object> arguments) {
            return switch (name) {
                case "getList", "getExtraButtonGroup" -> this;
                case "get_dataSource" -> products;
                case "getKinds" -> kinds;
                case "exists" -> buttons.containsKey(arguments.get(0));
                case "append" -> {
                    int index = (Integer)arguments.get(0);
                    require(!buttons.containsKey(index), "按钮重复注册");
                    Button button = new Button(this);
                    kinds.add(index);
                    buttons.put(index, button);
                    yield button;
                }
                case "remove" -> {
                    Integer index = (Integer)arguments.get(0);
                    require(buttons.remove(index) != null && kinds.remove(index), "删除了不存在的按钮");
                    yield this;
                }
                case "refreshAllCells" -> {
                    Set<Integer> expected = new java.util.HashSet<>();
                    for (int index = 0; index < products.size(); index += 1) {
                        if (products.get(index) == 1) { expected.add(index); }
                    }
                    require(buttons.keySet().equals(expected), "绘制前的横幅按钮集合不同");
                    require(buttons.values().stream().allMatch(button -> button.callbackOwner == this), "按钮点击回调丢失归属");
                    renders += 1;
                    yield this;
                }
                default -> throw new IllegalStateException("未覆盖的商店边界: " + name);
            };
        }
    }
    // //// /回放月卡页和索引四横幅页之间的切换 ////

    // //// 执行生成方法的属性访问, 原生边界和跳转指令 [@x380kkm 2026-09-28] ////
    private static void executeRefresh(LocalPaymentAbcPatch.ClassView owner, StoreScene scene) {
        var instructions = owner.method("refreshList").getCode().code;
        Map<Long, Integer> positions = new HashMap<>();
        for (int index = 0; index < instructions.size(); index += 1) {
            positions.put(instructions.get(index).getAddress(), index);
        }
        Object[] locals = new Object[8];
        locals[0] = scene;
        ArrayDeque<Object> stack = new ArrayDeque<>();
        for (int pc = 0, executed = 0; pc < instructions.size(); executed += 1) {
            require(executed < 10000, "商店指令跳转没有结束");
            var instruction = instructions.get(pc++);
            String opcode = instruction.definition.instructionName;
            boolean jump = false;
            switch (opcode) {
                case "getlocal0", "getlocal1", "getlocal2", "getlocal3" -> stack.push(locals[opcode.charAt(8) - '0']);
                case "getlocal" -> stack.push(locals[instruction.operands[0]]);
                case "setlocal0", "setlocal1", "setlocal2", "setlocal3" -> locals[opcode.charAt(8) - '0'] = stack.pop();
                case "setlocal" -> locals[instruction.operands[0]] = stack.pop();
                case "pushscope" -> stack.pop();
                case "pushbyte" -> stack.push(instruction.operands[0]);
                case "coerce" -> { }
                case "convert_i" -> stack.push(((Number)stack.pop()).intValue());
                case "declocal_i" -> locals[instruction.operands[0]] = (Integer)locals[instruction.operands[0]] - 1;
                case "inclocal_i" -> locals[instruction.operands[0]] = (Integer)locals[instruction.operands[0]] + 1;
                case "getproperty" -> {
                    var property = owner.abc().constants.getMultiname(instruction.operands[0]);
                    if (property.needsName()) {
                        int index = (Integer)stack.pop();
                        stack.push(((List<?>)stack.pop()).get(index));
                    } else {
                        String name = owner.abc().constants.getString(property.name_index);
                        Object receiver = stack.pop();
                        stack.push(switch (name) {
                            case "peek", "dialogListView", "index" -> receiver;
                            case "length" -> ((List<?>)receiver).size();
                            default -> throw new IllegalStateException("未覆盖的商店属性: " + name);
                        });
                    }
                }
                case "callproperty", "callpropvoid" -> {
                    String name = owner.abc().constants.getString(owner.abc().constants.getMultiname(instruction.operands[0]).name_index);
                    List<Object> arguments = new ArrayList<>();
                    for (int count = instruction.operands[1]; count > 0; count -= 1) { arguments.add(0, stack.pop()); }
                    Object receiver = stack.pop();
                    Object result = name.equals("get_displayKind") ? receiver : ((StoreScene)receiver).call(name, arguments);
                    if (opcode.equals("callproperty")) { stack.push(result); }
                }
                case "ifle", "iflt", "ifge", "ifeq", "ifne" -> {
                    int right = (Integer)stack.pop();
                    int left = (Integer)stack.pop();
                    jump = switch (opcode) {
                        case "ifle" -> left <= right;
                        case "iflt" -> left < right;
                        case "ifge" -> left >= right;
                        case "ifeq" -> left == right;
                        default -> left != right;
                    };
                }
                case "iftrue" -> jump = (Boolean)stack.pop();
                case "jump" -> jump = true;
                case "returnvoid" -> { require(stack.isEmpty(), "刷新方法遗留操作数"); return; }
                default -> throw new IllegalStateException("未覆盖的商店指令: " + opcode);
            }
            if (jump) { pc = positions.get(instruction.getTargetAddress()); }
        }
        throw new IllegalStateException("商店刷新方法缺少返回");
    }
    // //// /执行生成方法的属性访问, 原生边界和跳转指令 ////
}
