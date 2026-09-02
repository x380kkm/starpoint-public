// audience: internal
// # abc-method-reference-search
// 此工具在 SWF 的 AVM2 方法体中定位指定文本或常量引用.
// 运行前提是 Java 17 和 Starview 随附的 FFDec 库.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.types.ClassInfo;
import com.jpexs.decompiler.flash.abc.types.InstanceInfo;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.MethodInfo;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.abc.types.ScriptInfo;
import com.jpexs.decompiler.flash.abc.types.traits.Trait;
import com.jpexs.decompiler.flash.abc.types.traits.TraitFunction;
import com.jpexs.decompiler.flash.abc.types.traits.TraitMethodGetterSetter;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;

import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

public final class AbcMethodReferenceSearch {
    private AbcMethodReferenceSearch() {
    }

    // //// 加载 SWF 并输出匹配方法 [@x380kkm 2026-08-10] ////
    public static void main(String[] arguments) throws Exception {
        if (arguments.length < 2) {
            throw new IllegalArgumentException(
                    "usage: AbcMethodReferenceSearch <input.swf> <term> [term ...]");
        }

        Path inputPath = Path.of(arguments[0]).toAbsolutePath().normalize();
        if (!Files.isRegularFile(inputPath)) {
            throw new IllegalArgumentException("SWF does not exist: " + inputPath);
        }
        List<String> terms = List.of(arguments).subList(1, arguments.length);
        Set<MethodMatch> matches = new TreeSet<>();

        try (InputStream input = Files.newInputStream(inputPath)) {
            SWF swf = new SWF(input, false);
            int abcIndex = 0;
            for (ABCContainerTag tag : swf.getAbcList()) {
                ABC abc = tag.getABC();
                Map<Integer, MethodOwner> owners = indexMethodOwners(abc, abcIndex);
                for (MethodBody body : abc.bodies) {
                    String assembly = body.getCode().toASMSource(abc);
                    for (String term : terms) {
                        if (!assembly.contains(term)) {
                            continue;
                        }
                        MethodOwner owner = owners.getOrDefault(
                                body.method_info,
                                fallbackOwner(abc, abcIndex, body.method_info));
                        matches.add(new MethodMatch(owner.className(), owner.methodName(), term));
                    }
                }
                abcIndex++;
            }
        }

        System.out.println("matches=" + matches.size());
        for (MethodMatch match : matches) {
            System.out.println(match.className() + "\t" + match.methodName() + "\t" + match.term());
        }
    }
    // //// /加载 SWF 并输出匹配方法 ////

    // //// 建立方法索引到类和方法名称的映射 [@x380kkm 2026-08-10] ////
    private static Map<Integer, MethodOwner> indexMethodOwners(ABC abc, int abcIndex) {
        Map<Integer, MethodOwner> owners = new LinkedHashMap<>();
        for (int classIndex = 0; classIndex < abc.instance_info.size(); classIndex++) {
            InstanceInfo instanceInfo = abc.instance_info.get(classIndex);
            ClassInfo classInfo = abc.class_info.get(classIndex);
            String className = multiname(abc, instanceInfo.name_index);
            owners.put(instanceInfo.iinit_index, new MethodOwner(className, "<iinit>"));
            owners.put(classInfo.cinit_index, new MethodOwner(className, "<cinit>"));
            indexTraits(abc, className, instanceInfo.instance_traits.traits, owners);
            indexTraits(abc, className, classInfo.static_traits.traits, owners);
        }

        for (int scriptIndex = 0; scriptIndex < abc.script_info.size(); scriptIndex++) {
            ScriptInfo scriptInfo = abc.script_info.get(scriptIndex);
            String scriptName = "<abc-" + abcIndex + "-script-" + scriptIndex + ">";
            owners.put(scriptInfo.init_index, new MethodOwner(scriptName, "<init>"));
            indexTraits(abc, scriptName, scriptInfo.traits.traits, owners);
        }
        return owners;
    }
    // //// /建立方法索引到类和方法名称的映射 ////

    // //// 将 trait 方法加入方法索引 [@x380kkm 2026-08-10] ////
    private static void indexTraits(
            ABC abc,
            String className,
            List<Trait> traits,
            Map<Integer, MethodOwner> owners) {
        for (Trait trait : traits) {
            Integer methodInfo = methodInfo(trait);
            if (methodInfo == null) {
                continue;
            }
            owners.putIfAbsent(methodInfo, new MethodOwner(className, traitName(abc, trait)));
        }
    }
    // //// /将 trait 方法加入方法索引 ////

    private static Integer methodInfo(Trait trait) {
        if (trait instanceof TraitMethodGetterSetter method) {
            return method.method_info;
        }
        if (trait instanceof TraitFunction function) {
            return function.method_info;
        }
        return null;
    }

    private static String traitName(ABC abc, Trait trait) {
        Multiname multiname = abc.constants.getMultiname(trait.name_index);
        return multiname == null || !multiname.hasOwnName()
                ? "<anonymous-trait>"
                : abc.constants.getString(multiname.name_index);
    }

    private static String multiname(ABC abc, int multinameIndex) {
        Multiname multiname = abc.constants.getMultiname(multinameIndex);
        return multiname == null
                ? "<anonymous-class>"
                : multiname.getNameWithNamespace(abc.constants, false).toRawString();
    }

    private static MethodOwner fallbackOwner(ABC abc, int abcIndex, int methodIndex) {
        MethodInfo methodInfo = abc.method_info.get(methodIndex);
        String methodName = abc.constants.getString(methodInfo.name_index);
        if (methodName == null || methodName.isBlank()) {
            methodName = "<method-info-" + methodIndex + ">";
        }
        return new MethodOwner("<abc-" + abcIndex + ">", methodName);
    }

    private record MethodOwner(String className, String methodName) {
    }

    private record MethodMatch(String className, String methodName, String term)
            implements Comparable<MethodMatch> {
        @Override
        public int compareTo(MethodMatch other) {
            int classOrder = className.compareTo(other.className);
            if (classOrder != 0) {
                return classOrder;
            }
            int methodOrder = methodName.compareTo(other.methodName);
            return methodOrder != 0 ? methodOrder : term.compareTo(other.term);
        }
    }
}
