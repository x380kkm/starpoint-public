// audience: internal
// # abc-method-digest
// 此工具计算 AVM2 类的静态或实例方法摘要.
// 运行前提是 Java 17 和 Starview 随附的 FFDec 库.
// 摘要覆盖方法定义, 方法体, 引用常量语义, 异常表和方法内 trait.

import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.types.ABCException;
import com.jpexs.decompiler.flash.abc.types.ClassInfo;
import com.jpexs.decompiler.flash.abc.types.InstanceInfo;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.MethodInfo;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.abc.types.ValueKind;
import com.jpexs.decompiler.flash.abc.types.traits.Trait;
import com.jpexs.decompiler.flash.abc.types.traits.TraitFunction;
import com.jpexs.decompiler.flash.abc.types.traits.TraitMethodGetterSetter;

import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.LinkedHashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

final class AbcMethodDigest {
    static final int VERSION = 2;

    // //// 阻止实例化通用摘要器 [@x380kkm 2026-08-11] ////
    private AbcMethodDigest() {
    }
    // //// /阻止实例化通用摘要器 ////

    // //// 计算全部静态方法和类初始化器摘要 [@x380kkm 2026-08-11] ////
    static Map<String, String> digestStaticMethods(ABC abc, int classIndex) throws Exception {
        ClassInfo classInfo = abc.class_info.get(classIndex);
        return digestMethods(
                abc,
                classInfo.cinit_index,
                "<cinit>",
                classInfo.static_traits.traits);
    }
    // //// /计算全部静态方法和类初始化器摘要 ////

    // //// 计算全部实例方法和实例初始化器摘要 [@x380kkm 2026-08-11] ////
    static Map<String, String> digestInstanceMethods(ABC abc, int classIndex) throws Exception {
        InstanceInfo instanceInfo = abc.instance_info.get(classIndex);
        return digestMethods(
                abc,
                instanceInfo.iinit_index,
                "<iinit>",
                instanceInfo.instance_traits.traits);
    }
    // //// /计算全部实例方法和实例初始化器摘要 ////

    // //// 查找指定静态方法 [@x380kkm 2026-08-11] ////
    static MethodBody findStaticMethod(ABC abc, int classIndex, String methodName) {
        ClassInfo classInfo = abc.class_info.get(classIndex);
        return findMethod(abc, classInfo.static_traits.traits, methodName, "static");
    }
    // //// /查找指定静态方法 ////

    // //// 查找指定实例方法 [@x380kkm 2026-08-11] ////
    static MethodBody findInstanceMethod(ABC abc, int classIndex, String methodName) {
        InstanceInfo instanceInfo = abc.instance_info.get(classIndex);
        return findMethod(abc, instanceInfo.instance_traits.traits, methodName, "instance");
    }
    // //// /查找指定实例方法 ////

    // //// 计算方法签名, 方法体和引用常量语义摘要 [@x380kkm 2026-08-11] ////
    static String digestMethod(ABC abc, Trait ownerTrait, MethodBody body) throws Exception {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        try (DataOutputStream output = new DataOutputStream(bytes)) {
            output.writeInt(VERSION);
            writeTrait(output, abc, ownerTrait);
            writeMethodInfo(output, abc, abc.method_info.get(body.method_info));

            output.writeInt(body.max_stack);
            output.writeInt(body.max_regs);
            output.writeInt(body.init_scope_depth);
            output.writeInt(body.max_scope_depth);

            byte[] code = body.getCodeBytes();
            output.writeInt(code.length);
            output.write(code);
            writeString(output, body.getCode().toASMSource(abc));

            ABCException[] exceptions = body.exceptions == null ? new ABCException[0] : body.exceptions;
            output.writeInt(exceptions.length);
            for (ABCException exception : exceptions) {
                output.writeInt(exception.start);
                output.writeInt(exception.end);
                output.writeInt(exception.target);
                output.writeInt(exception.type_index);
                output.writeInt(exception.name_index);
                writeMultiname(output, abc, exception.type_index);
                writeMultiname(output, abc, exception.name_index);
            }

            List<Trait> traits = body.traits == null ? List.of() : body.traits.traits;
            output.writeInt(traits.size());
            for (Trait trait : traits) {
                output.writeUTF(trait.getClass().getName());
                output.writeInt(trait.name_index);
                output.writeInt(trait.kindType);
                output.writeInt(trait.kindFlags);
                int[] metadata = trait.metadata == null ? new int[0] : trait.metadata;
                output.writeInt(metadata.length);
                for (int value : metadata) {
                    output.writeInt(value);
                }
                byte[] traitBytes = trait.bytes == null ? new byte[0] : trait.bytes;
                output.writeInt(traitBytes.length);
                output.write(traitBytes);
            }
        }
        return sha256(bytes.toByteArray());
    }
    // //// /计算方法签名, 方法体和引用常量语义摘要 ////

    // //// 返回摘要发生变化的方法 [@x380kkm 2026-08-11] ////
    static Set<String> changedMethods(Map<String, String> before, Map<String, String> after) {
        if (!before.keySet().equals(after.keySet())) {
            throw new IllegalStateException(
                    "method set changed: before=" + before.keySet() + " after=" + after.keySet());
        }
        Set<String> changed = new TreeSet<>();
        for (String method : before.keySet()) {
            if (!before.get(method).equals(after.get(method))) {
                changed.add(method);
            }
        }
        return changed;
    }
    // //// /返回摘要发生变化的方法 ////

    // //// 要求全部方法摘要保持一致 [@x380kkm 2026-08-11] ////
    static void requireSameMethods(
            Map<String, String> expected,
            Map<String, String> actual,
            String message) {
        if (!expected.equals(actual)) {
            throw new IllegalStateException(message + ": " + changedMethods(expected, actual));
        }
    }
    // //// /要求全部方法摘要保持一致 ////

    // //// 核对实例方法及重编号闭包的完整内容 [@x380kkm 2026-09-26] ////
    static void requireSameInstanceMethods(
            ABC expected, int expectedClass, ABC actual, int actualClass, String message) throws Exception {
        InstanceInfo expectedInfo = expected.instance_info.get(expectedClass);
        InstanceInfo actualInfo = actual.instance_info.get(actualClass);
        Map<String, MethodDefinition> expectedMethods = collectMethods(
                expected, expectedInfo.iinit_index, "<iinit>", expectedInfo.instance_traits.traits);
        Map<String, MethodDefinition> actualMethods = collectMethods(
                actual, actualInfo.iinit_index, "<iinit>", actualInfo.instance_traits.traits);
        if (!expectedMethods.keySet().equals(actualMethods.keySet())) {
            throw new IllegalStateException(message + ": 方法集合不同");
        }
        for (String name : expectedMethods.keySet()) {
            requireSameMethod(expected, expectedMethods.get(name), actual, actualMethods.get(name),
                    new HashSet<>(), message + ": " + name);
        }
    }

    private static void requireSameMethod(ABC expected, MethodDefinition expectedMethod,
            ABC actual, MethodDefinition actualMethod, Set<MethodPair> visited, String message) throws Exception {
        MethodBody expectedBody = expectedMethod.body();
        MethodBody actualBody = actualMethod.body();
        MethodPair pair = new MethodPair(expectedBody.method_info, actualBody.method_info);
        if (!visited.add(pair)) { return; }
        List<AVM2Instruction> expectedCode = expectedBody.getCode().code;
        List<AVM2Instruction> actualCode = actualBody.getCode().code;
        if (expectedCode.size() != actualCode.size()) { throw new IllegalStateException(message); }
        byte[] normalized = expectedBody.getCodeBytes().clone();
        for (int index = 0; index < expectedCode.size(); index++) {
            AVM2Instruction before = expectedCode.get(index);
            if (!before.definition.instructionName.equals("newfunction")) { continue; }
            AVM2Instruction after = actualCode.get(index);
            if (!after.definition.instructionName.equals("newfunction")
                    || before.getAddress() != after.getAddress()
                    || before.getBytesLength() != after.getBytesLength()) {
                throw new IllegalStateException(message + ": 闭包指令边界不同");
            }
            MethodBody expectedClosure = expected.findBody(before.operands[0]);
            MethodBody actualClosure = actual.findBody(after.operands[0]);
            if (expectedClosure == null || actualClosure == null) {
                throw new IllegalStateException(message + ": 闭包方法体缺失");
            }
            requireSameMethod(expected, new MethodDefinition(null, expectedClosure),
                    actual, new MethodDefinition(null, actualClosure), visited, message + ": 闭包");
            byte[] reference = after.getBytes();
            System.arraycopy(reference, 0, normalized, Math.toIntExact(before.getAddress()), reference.length);
        }
        MethodBody comparable = expectedBody.clone();
        comparable.setCodeBytes(normalized);
        if (!digestMethod(expected, expectedMethod.trait(), comparable)
                .equals(digestMethod(actual, actualMethod.trait(), actualBody))) {
            throw new IllegalStateException(message);
        }
    }

    private record MethodDefinition(Trait trait, MethodBody body) { }
    private record MethodPair(int expected, int actual) { }
    // //// /核对实例方法及重编号闭包的完整内容 ////

    // //// 计算指定 trait 集合中的方法摘要 [@x380kkm 2026-08-11] ////
    private static Map<String, String> digestMethods(
            ABC abc,
            int initializerIndex,
            String initializerName,
            List<Trait> traits) throws Exception {
        Map<String, String> digests = new LinkedHashMap<>();
        for (var entry : collectMethods(abc, initializerIndex, initializerName, traits).entrySet()) {
            MethodDefinition method = entry.getValue();
            digests.put(entry.getKey(), digestMethod(abc, method.trait(), method.body()));
        }
        return digests;
    }

    private static Map<String, MethodDefinition> collectMethods(
            ABC abc, int initializerIndex, String initializerName, List<Trait> traits) {
        Map<String, MethodDefinition> methods = new LinkedHashMap<>();
        MethodBody initializer = abc.findBody(initializerIndex);
        if (initializer == null) {
            throw new IllegalStateException("method initializer is missing: " + initializerName);
        }
        methods.put(initializerName, new MethodDefinition(null, initializer));

        for (Trait trait : traits) {
            Integer methodInfo = methodInfo(trait);
            if (methodInfo == null) {
                continue;
            }
            MethodBody body = abc.findBody(methodInfo);
            if (body == null) {
                continue;
            }
            String methodName = traitName(abc, trait);
            if (methods.put(methodName, new MethodDefinition(trait, body)) != null) {
                throw new IllegalStateException("duplicate method name: " + methodName);
            }
        }
        return methods;
    }
    // //// /计算指定 trait 集合中的方法摘要 ////

    // //// 在指定 trait 集合中查找方法 [@x380kkm 2026-08-11] ////
    private static MethodBody findMethod(
            ABC abc,
            List<Trait> traits,
            String methodName,
            String methodKind) {
        for (Trait trait : traits) {
            Integer methodInfo = methodInfo(trait);
            if (methodInfo != null && traitName(abc, trait).equals(methodName)) {
                MethodBody body = abc.findBody(methodInfo);
                if (body != null) {
                    return body;
                }
            }
        }
        throw new IllegalStateException(methodKind + " method is missing: " + methodName);
    }
    // //// /在指定 trait 集合中查找方法 ////

    // //// 写入方法定义语义 [@x380kkm 2026-08-11] ////
    private static void writeMethodInfo(DataOutputStream output, ABC abc, MethodInfo method) throws Exception {
        output.writeBoolean(method.deleted);
        output.writeInt(method.flags);
        writeMultiname(output, abc, method.ret_type);
        writeString(output, abc.constants.getString(method.name_index));

        int[] parameterTypes = method.param_types == null ? new int[0] : method.param_types;
        output.writeInt(parameterTypes.length);
        for (int parameterType : parameterTypes) {
            writeMultiname(output, abc, parameterType);
        }

        ValueKind[] optionalValues = method.optional == null ? new ValueKind[0] : method.optional;
        output.writeInt(optionalValues.length);
        for (ValueKind optionalValue : optionalValues) {
            output.writeInt(optionalValue.value_kind);
            writeString(output, optionalValue.toASMString(abc));
        }

        int[] parameterNames = method.paramNames == null ? new int[0] : method.paramNames;
        output.writeInt(parameterNames.length);
        for (int parameterName : parameterNames) {
            writeString(output, abc.constants.getString(parameterName));
        }
    }
    // //// /写入方法定义语义 ////

    // //// 写入方法所属 trait 语义 [@x380kkm 2026-08-11] ////
    private static void writeTrait(DataOutputStream output, ABC abc, Trait trait) throws Exception {
        output.writeBoolean(trait != null);
        if (trait == null) {
            return;
        }

        writeString(output, trait.getClass().getName());
        output.writeBoolean(trait.deleted);
        output.writeInt(trait.kindType);
        output.writeInt(trait.kindFlags);
        writeMultiname(output, abc, trait.name_index);
        int[] metadata = trait.metadata == null ? new int[0] : trait.metadata;
        output.writeInt(metadata.length);
        for (int metadataIndex : metadata) {
            writeString(output, abc.metadata_info.get(metadataIndex).toString(abc.constants));
        }
        if (trait instanceof TraitMethodGetterSetter method) {
            output.writeInt(method.disp_id);
        } else if (trait instanceof TraitFunction function) {
            output.writeInt(function.slot_id);
        }
    }
    // //// /写入方法所属 trait 语义 ////

    // //// 写入 multiname 语义 [@x380kkm 2026-08-11] ////
    private static void writeMultiname(DataOutputStream output, ABC abc, int multinameIndex) throws Exception {
        Multiname multiname = abc.constants.getMultiname(multinameIndex);
        writeString(output, multiname == null ? null : multiname.toString(abc.constants, List.of()));
    }
    // //// /写入 multiname 语义 ////

    // //// 写入不限长度的 UTF-8 文本 [@x380kkm 2026-08-11] ////
    private static void writeString(DataOutputStream output, String value) throws Exception {
        if (value == null) {
            output.writeInt(-1);
            return;
        }
        byte[] valueBytes = value.getBytes(StandardCharsets.UTF_8);
        output.writeInt(valueBytes.length);
        output.write(valueBytes);
    }
    // //// /写入不限长度的 UTF-8 文本 ////

    // //// 读取 trait 方法索引 [@x380kkm 2026-08-11] ////
    private static Integer methodInfo(Trait trait) {
        if (trait instanceof TraitMethodGetterSetter method) {
            return method.method_info;
        }
        if (trait instanceof TraitFunction function) {
            return function.method_info;
        }
        return null;
    }
    // //// /读取 trait 方法索引 ////

    // //// 读取 trait 名称 [@x380kkm 2026-08-11] ////
    private static String traitName(ABC abc, Trait trait) {
        Multiname multiname = abc.constants.getMultiname(trait.name_index);
        if (multiname == null || !multiname.hasOwnName()) {
            throw new IllegalStateException("method trait name is missing: " + trait.name_index);
        }
        return abc.constants.getString(multiname.name_index);
    }
    // //// /读取 trait 名称 ////

    // //// 计算 SHA-256 摘要 [@x380kkm 2026-08-11] ////
    private static String sha256(byte[] value) throws Exception {
        byte[] digest = MessageDigest.getInstance("SHA-256").digest(value);
        StringBuilder result = new StringBuilder(digest.length * 2);
        for (byte item : digest) {
            result.append(String.format("%02x", item));
        }
        return result.toString();
    }
    // //// /计算 SHA-256 摘要 ////
}
