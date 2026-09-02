// audience: internal
// # remote-util-method-digest
// 此工具保留 RemoteUtil 摘要 API 并验证两个协议关键方法存在.
// 运行前提是 Java 17, AbcMethodDigest 和 Starview 随附的 FFDec 库.

import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.traits.Trait;

import java.util.Map;
import java.util.Set;

final class RemoteUtilMethodDigest {
    static final int VERSION = AbcMethodDigest.VERSION;

    // //// 阻止实例化 RemoteUtil 摘要包装器 [@x380kkm 2026-08-11] ////
    private RemoteUtilMethodDigest() {
    }
    // //// /阻止实例化 RemoteUtil 摘要包装器 ////

    // //// 计算 RemoteUtil 静态方法摘要并验证协议方法 [@x380kkm 2026-08-11] ////
    static Map<String, String> digestStaticMethods(ABC abc, int classIndex) throws Exception {
        Map<String, String> digests = AbcMethodDigest.digestStaticMethods(abc, classIndex);
        if (!digests.containsKey("getURLRequest") || !digests.containsKey("requestCompleteHandler")) {
            throw new IllegalStateException("RemoteUtil required methods are missing: " + digests.keySet());
        }
        return digests;
    }
    // //// /计算 RemoteUtil 静态方法摘要并验证协议方法 ////

    // //// 查找 RemoteUtil 静态方法 [@x380kkm 2026-08-11] ////
    static MethodBody findStaticMethod(ABC abc, int classIndex, String methodName) {
        return AbcMethodDigest.findStaticMethod(abc, classIndex, methodName);
    }
    // //// /查找 RemoteUtil 静态方法 ////

    // //// 计算单个 RemoteUtil 方法摘要 [@x380kkm 2026-08-11] ////
    static String digestMethod(ABC abc, Trait ownerTrait, MethodBody body) throws Exception {
        return AbcMethodDigest.digestMethod(abc, ownerTrait, body);
    }
    // //// /计算单个 RemoteUtil 方法摘要 ////

    // //// 返回 RemoteUtil 摘要发生变化的方法 [@x380kkm 2026-08-11] ////
    static Set<String> changedMethods(Map<String, String> before, Map<String, String> after) {
        return AbcMethodDigest.changedMethods(before, after);
    }
    // //// /返回 RemoteUtil 摘要发生变化的方法 ////

    // //// 要求 RemoteUtil 全部方法摘要保持一致 [@x380kkm 2026-08-11] ////
    static void requireSameMethods(
            Map<String, String> expected,
            Map<String, String> actual,
            String message) {
        AbcMethodDigest.requireSameMethods(expected, actual, message);
    }
    // //// /要求 RemoteUtil 全部方法摘要保持一致 ////
}
