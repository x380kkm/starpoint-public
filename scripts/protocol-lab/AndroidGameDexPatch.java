// audience: internal
// # android-game-dex-patch
//
// dexlib2 保留单 DEX 内的类和方法结构, 并在壳 Application 的正常返回处安装宿主连接.
// 输入来自 SDK 初始化补丁后的游戏 DEX 与 D8 生成的宿主连接 DEX.

import com.android.tools.smali.dexlib2.AccessFlags;
import com.android.tools.smali.dexlib2.DexFileFactory;
import com.android.tools.smali.dexlib2.Opcode;
import com.android.tools.smali.dexlib2.builder.MutableMethodImplementation;
import com.android.tools.smali.dexlib2.builder.instruction.BuilderInstruction10x;
import com.android.tools.smali.dexlib2.builder.instruction.BuilderInstruction3rc;
import com.android.tools.smali.dexlib2.iface.ClassDef;
import com.android.tools.smali.dexlib2.iface.DexFile;
import com.android.tools.smali.dexlib2.iface.Method;
import com.android.tools.smali.dexlib2.iface.instruction.Instruction;
import com.android.tools.smali.dexlib2.iface.instruction.ReferenceInstruction;
import com.android.tools.smali.dexlib2.iface.reference.MethodReference;
import com.android.tools.smali.dexlib2.immutable.ImmutableClassDef;
import com.android.tools.smali.dexlib2.immutable.ImmutableDexFile;
import com.android.tools.smali.dexlib2.immutable.ImmutableMethod;
import com.android.tools.smali.dexlib2.immutable.reference.ImmutableMethodReference;
import com.google.gson.GsonBuilder;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class AndroidGameDexPatch {
    private static final String APPLICATION = "Ls/h/e/l/l/S;";
    private static final String CONNECTION = "Ldev/starpoint/gamehost/GameHostConnection;";
    private static final ImmutableMethodReference INSTALL = new ImmutableMethodReference(
        CONNECTION, "install", List.of("Landroid/app/Application;"), "V"
    );

    // //// 合并游戏和宿主连接类并写入定位报告 [@x380kkm 2026-09-10] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException("参数: game.dex bridge.dex output.dex report.json");
        }
        requireStandardDex(Path.of(args[0]));
        DexFile source = DexFileFactory.loadDexFile(args[0], null);
        DexFile bridge = DexFileFactory.loadDexFile(args[1], null);
        Map<String, ClassDef> classes = collectClasses(source);
        ClassDef application = classes.get(APPLICATION);
        if (application == null) {
            throw new IllegalArgumentException("游戏 DEX 缺少 " + APPLICATION);
        }
        requireInstallMethod(bridge);
        for (ClassDef definition : bridge.getClasses()) {
            if (classes.putIfAbsent(definition.getType(), definition) != null) {
                throw new IllegalArgumentException("DEX 类碰撞或重复注入: " + definition.getType());
            }
        }
        Map<String, Object> report = new LinkedHashMap<>();
        classes.put(APPLICATION, injectConnection(application, report));
        DexFile output = new ImmutableDexFile(source.getOpcodes(), classes.values());
        DexFileFactory.writeDexFile(args[2], output);
        report.put("inputClasses", source.getClasses().size());
        report.put("bridgeClasses", bridge.getClasses().size());
        report.put("outputClasses", classes.size());
        Files.writeString(Path.of(args[3]), new GsonBuilder().setPrettyPrinting().create().toJson(report),
            StandardCharsets.UTF_8);
        System.out.println("DEX 宿主连接: " + APPLICATION + "->onCreate()V, 类数=" + classes.size());
    }

    // //// 验证 DEX 末尾附加数据边界 [@x380kkm 2026-09-10] ////
    private static void requireStandardDex(Path path) throws Exception {
        byte[] bytes = Files.readAllBytes(path);
        if (bytes.length < 112 || ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).getInt(32) != bytes.length) {
            throw new IllegalArgumentException("DEX 长度与头部边界不一致: " + path);
        }
        ByteBuffer header = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
        if (header.getInt(44) != 0 || header.getInt(48) != 0) {
            throw new IllegalArgumentException("DEX 包含独立链接数据: " + path);
        }
    }

    // //// 收集唯一类定义 [@x380kkm 2026-09-10] ////
    private static Map<String, ClassDef> collectClasses(DexFile dex) {
        Map<String, ClassDef> classes = new LinkedHashMap<>();
        for (ClassDef definition : dex.getClasses()) {
            if (classes.putIfAbsent(definition.getType(), definition) != null) {
                throw new IllegalArgumentException("DEX 存在重复类: " + definition.getType());
            }
        }
        return classes;
    }

    // //// 验证连接安装方法的公开静态签名 [@x380kkm 2026-09-10] ////
    private static void requireInstallMethod(DexFile bridge) {
        int matches = 0;
        for (ClassDef definition : bridge.getClasses()) {
            for (Method method : definition.getMethods()) {
                if (INSTALL.equals(method)) {
                    if (!AccessFlags.PUBLIC.isSet(method.getAccessFlags())
                        || !AccessFlags.STATIC.isSet(method.getAccessFlags())) {
                        throw new IllegalArgumentException("连接安装方法需要 public static: " + method);
                    }
                    matches++;
                }
            }
        }
        if (matches != 1) {
            throw new IllegalArgumentException("连接安装方法数量错误: " + matches);
        }
    }

    // //// 替换唯一 Application 方法并保留其余成员 [@x380kkm 2026-09-10] ////
    private static ClassDef injectConnection(ClassDef definition, Map<String, Object> report) {
        List<Method> methods = new ArrayList<>();
        int targets = 0;
        for (Method method : definition.getMethods()) {
            if (method.getName().equals("onCreate") && method.getParameterTypes().isEmpty()
                && method.getReturnType().equals("V")) {
                methods.add(injectInstallCall(method, report));
                targets++;
            } else {
                methods.add(method);
            }
        }
        if (targets != 1) {
            throw new IllegalArgumentException("Application onCreate 目标数量错误: " + targets);
        }
        return new ImmutableClassDef(definition.getType(), definition.getAccessFlags(),
            definition.getSuperclass(), definition.getInterfaces(), definition.getSourceFile(),
            definition.getAnnotations(), definition.getFields(), methods);
    }

    // //// 在正常返回位置保留分支标签并安装连接 [@x380kkm 2026-09-10] ////
    private static Method injectInstallCall(Method method, Map<String, Object> report) {
        if (method.getImplementation() == null || AccessFlags.STATIC.isSet(method.getAccessFlags())) {
            throw new IllegalArgumentException("Application onCreate 方法体无效: " + method);
        }
        MutableMethodImplementation implementation = new MutableMethodImplementation(method.getImplementation());
        List<Integer> returns = new ArrayList<>();
        int address = 0;
        int returnAddress = -1;
        for (int index = 0; index < implementation.getInstructions().size(); index++) {
            Instruction instruction = implementation.getInstructions().get(index);
            if (instruction instanceof ReferenceInstruction reference
                && reference.getReference() instanceof MethodReference called
                && called.getDefiningClass().equals(CONNECTION)) {
                throw new IllegalArgumentException("Application 已包含连接调用: 地址=" + address);
            }
            if (instruction.getOpcode() == Opcode.RETURN_VOID) {
                returns.add(index);
                returnAddress = address;
            }
            address += instruction.getCodeUnits();
        }
        if (returns.size() != 1) {
            throw new IllegalArgumentException("Application 正常返回数量错误: " + returns.size());
        }
        int applicationRegister = implementation.getRegisterCount() - 1;
        int returnIndex = returns.get(0);
        implementation.replaceInstruction(returnIndex, new BuilderInstruction3rc(
            Opcode.INVOKE_STATIC_RANGE, applicationRegister, 1, INSTALL));
        implementation.addInstruction(returnIndex + 1, new BuilderInstruction10x(Opcode.RETURN_VOID));
        report.put("target", APPLICATION + "->onCreate()V");
        report.put("returnCodeAddress", returnAddress);
        report.put("applicationRegister", applicationRegister);
        report.put("injectedCalls", 1);
        return new ImmutableMethod(method.getDefiningClass(), method.getName(), method.getParameters(),
            method.getReturnType(), method.getAccessFlags(), method.getAnnotations(),
            method.getHiddenApiRestrictions(), implementation);
    }
}
