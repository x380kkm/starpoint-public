// audience: internal
// # export-asset-path-constants
// 此工具通过 FFDec 读取实际 SWF 的路径字符串常量, 输出包含字节码常量索引的 UTF-8 文本.
// 运行前提是 Java 17 和 FFDec 库.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.TreeSet;

public final class ExportAssetPathConstants {
    // //// 提取 SWF 内的资源路径常量 [@x380kkm 2026-09-21] ////
    public static void main(String[] arguments) throws Exception {
        var paths = new TreeSet<String>();
        try (var input = Files.newInputStream(Path.of(arguments[0]))) {
            var swf = new SWF(input, false);
            int abcIndex = 0;
            for (ABCContainerTag tag : swf.getAbcList()) {
                var constants = tag.getABC().constants;
                for (int index = 1; index < constants.getStringCount(); index++) {
                    String value = constants.getString(index);
                    if (value.matches("/?[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)+")) {
                        paths.add(value + "\t" + abcIndex + ":" + index);
                    }
                }
                abcIndex++;
            }
        }
        Files.write(Path.of(arguments[1]), paths, StandardCharsets.UTF_8);
        System.out.println("path_constants=" + paths.size());
    }
    // //// /提取 SWF 内的资源路径常量 ////
}
