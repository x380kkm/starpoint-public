// audience: internal
// # android-game-manifest-patch
//
// AXML 读写来自 Apache-2.0 许可的 de.upb.cs.swt:axml, https://github.com/Sable/axml.
// 修改二进制清单时保留属性类型, 资源引用, 命名空间和原组件, 桌面入口绑定宿主连接 Activity.
// AIR 按 AppEntry 的 uniqueappversionid 选择代码缓存目录, 该标识由 SWF 内容和游戏版本共同确定.

import com.google.gson.GsonBuilder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import pxb.android.axml.AxmlReader;
import pxb.android.axml.AxmlVisitor;
import pxb.android.axml.AxmlWriter;
import pxb.android.axml.NodeVisitor;

public final class AndroidGameManifestPatch {
    private static final String ANDROID = "http://schemas.android.com/apk/res/android";
    private static final String GATEWAY = "dev.starpoint.gamehost.GameLaunchActivity";
    private static final String GAME_ACTIVITY = "air.com.leiting.wf.AppEntry";
    private static final String HOST_PACKAGE = "dev.starpoint.personalservice";
    private static final int NAME_ID = 0x01010003;
    private static final int VALUE_ID = 0x01010024;
    private static final int VERSION_CODE_ID = 0x0101021b;

    // //// 改写游戏入口, 分发版本和 AIR 代码缓存身份 [@x380kkm 2026-09-21] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 6) {
            throw new IllegalArgumentException("参数: input.xml output.xml versionCode report.json game.swf releaseVersion");
        }
        Document document = read(Files.readAllBytes(Path.of(args[0])));
        Element manifest = requireSingle(document.roots, "manifest");
        if (!"com.leiting.wf".equals(manifest.value(null, "package"))) {
            throw new IllegalArgumentException("输入清单包名不匹配");
        }
        Element application = requireSingle(manifest.children, "application");
        Element originalGame = application.children.stream().filter(element ->
            element.name.equals("activity") && GAME_ACTIVITY.equals(element.value(ANDROID, "name")))
            .reduce((first, second) -> { throw new IllegalArgumentException("游戏 Activity 重复"); })
            .orElseThrow(() -> new IllegalArgumentException("游戏清单缺少 AppEntry"));
        if (!Integer.valueOf(1).equals(originalGame.value(ANDROID, "screenOrientation"))
            || !Integer.valueOf(0x40000da0).equals(originalGame.value(ANDROID, "configChanges"))) {
            throw new IllegalArgumentException("游戏 Activity 屏幕配置不匹配");
        }
        if (application.children.stream().anyMatch(element -> GATEWAY.equals(element.value(ANDROID, "name")))) {
            throw new IllegalArgumentException("清单已包含宿主连接入口");
        }
        int originalComponents = application.children.size();
        Attribute originalVersionName = manifest.attribute(ANDROID, "versionName");
        if (originalVersionName == null || !(originalVersionName.value instanceof String clientVersion)) {
            throw new IllegalArgumentException("游戏清单缺少文本版本名");
        }
        String versionName = createDistributionVersionName(clientVersion, args[5]);
        List<Element> launchers = removeLaunchers(application);
        if (launchers.size() != 1) {
            throw new IllegalArgumentException("原 LAUNCHER 过滤器数量错误: " + launchers.size());
        }
        application.children.add(createGateway(originalGame, launchers));
        addHostQuery(manifest);
        int versionCode = Integer.parseInt(args[2]);
        if (versionCode <= 0) {
            throw new IllegalArgumentException("游戏版本编号需要大于 0");
        }
        manifest.setAttribute(new Attribute(ANDROID, "versionCode", VERSION_CODE_ID,
            NodeVisitor.TYPE_FIRST_INT, versionCode));
        manifest.setAttribute(new Attribute(ANDROID, "versionName", originalVersionName.resourceId,
            NodeVisitor.TYPE_STRING, versionName));
        String uniqueAppVersionId = updateAirCacheIdentity(originalGame, Path.of(args[4]), versionCode);
        Files.write(Path.of(args[1]), write(document));
        Map<String, Object> report = new LinkedHashMap<>();
        report.put("package", "com.leiting.wf");
        report.put("launcherActivity", GATEWAY);
        report.put("gameActivity", GAME_ACTIVITY);
        report.put("hostPackageQuery", HOST_PACKAGE);
        report.put("versionCode", versionCode);
        report.put("versionName", versionName);
        report.put("releaseVersion", args[5]);
        report.put("clientVersionName", clientVersion);
        report.put("uniqueAppVersionId", uniqueAppVersionId);
        report.put("originalApplicationChildren", originalComponents);
        report.put("outputApplicationChildren", application.children.size());
        report.put("movedLauncherFilters", launchers.size());
        Files.writeString(Path.of(args[3]), new GsonBuilder().setPrettyPrinting().create().toJson(report),
            StandardCharsets.UTF_8);
        System.out.println("游戏清单: launcher=" + GATEWAY + ", versionCode=" + versionCode);
    }

    // //// 将分发版本映射到高于原客户端的应用版本 [@x380kkm 2026-09-21] ////
    private static String createDistributionVersionName(String clientVersion, String releaseVersion) {
        if (!clientVersion.matches("[0-9]+\\.[0-9]+\\.[0-9]+")
            || !releaseVersion.matches("[0-9]+\\.[0-9]+\\.[0-9]+")) {
            throw new IllegalArgumentException("客户端和分发版本需要使用三段数字");
        }
        String[] clientParts = clientVersion.split("\\.");
        String[] releaseParts = releaseVersion.split("\\.");
        int major = Math.addExact(Math.addExact(Integer.parseInt(clientParts[0]), 1),
            Integer.parseInt(releaseParts[0]));
        return major + "." + Integer.parseInt(releaseParts[1]) + "." + Integer.parseInt(releaseParts[2]);
    }

    // //// 将 AIR 代码缓存绑定到包内 SWF 和游戏版本 [@x380kkm 2026-09-20] ////
    private static String updateAirCacheIdentity(Element game, Path swf, int versionCode) throws Exception {
        Element metadata = game.children.stream().filter(element -> element.name.equals("meta-data")
            && "uniqueappversionid".equals(element.value(ANDROID, "name")))
            .reduce((first, second) -> { throw new IllegalArgumentException("AIR 缓存标识重复"); })
            .orElseThrow(() -> new IllegalArgumentException("游戏清单缺少 AIR 缓存标识"));
        String identity = createAirCacheIdentity(swf, versionCode);
        metadata.setAttribute(new Attribute(ANDROID, "value", VALUE_ID, NodeVisitor.TYPE_STRING, identity));
        return identity;
    }

    // //// 根据 SWF 内容和游戏版本生成稳定缓存标识 [@x380kkm 2026-09-20] ////
    private static String createAirCacheIdentity(Path swf, int versionCode) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        digest.update(Integer.toString(versionCode).getBytes(StandardCharsets.UTF_8));
        digest.update((byte) 0);
        try (var stream = Files.newInputStream(swf)) {
            byte[] buffer = new byte[65536];
            for (int length; (length = stream.read(buffer)) != -1;) {
                digest.update(buffer, 0, length);
            }
        }
        return UUID.nameUUIDFromBytes(digest.digest()).toString();
    }

    // //// 从原桌面组件转移完整启动过滤器 [@x380kkm 2026-09-10] ////
    private static List<Element> removeLaunchers(Element application) {
        List<Element> launchers = new ArrayList<>();
        for (Element component : application.children) {
            if (!component.name.equals("activity") && !component.name.equals("activity-alias")) {
                continue;
            }
            component.children.removeIf(filter -> {
                boolean launcher = filter.name.equals("intent-filter") && filter.children.stream().anyMatch(
                    child -> child.name.equals("category")
                        && "android.intent.category.LAUNCHER".equals(child.value(ANDROID, "name")));
                if (launcher) {
                    if (filter.children.stream().noneMatch(child -> child.name.equals("action")
                        && "android.intent.action.MAIN".equals(child.value(ANDROID, "name")))) {
                        throw new IllegalArgumentException("LAUNCHER 过滤器缺少 MAIN action");
                    }
                    launchers.add(filter);
                }
                return launcher;
            });
        }
        return launchers;
    }

    // //// 生成保留游戏屏幕配置的宿主连接入口 [@x380kkm 2026-09-10] ////
    private static Element createGateway(Element game, List<Element> launchers) {
        Element gateway = new Element(null, "activity");
        gateway.setAttribute(new Attribute(ANDROID, "name", NAME_ID, NodeVisitor.TYPE_STRING, GATEWAY));
        gateway.setAttribute(new Attribute(ANDROID, "exported", 0x01010010, NodeVisitor.TYPE_INT_BOOLEAN, true));
        gateway.setAttribute(new Attribute(ANDROID, "theme", 0x01010000, NodeVisitor.TYPE_REFERENCE, 0x01030241));
        for (String name : List.of("screenOrientation", "configChanges", "resizeableActivity")) {
            Attribute attribute = game.attribute(ANDROID, name);
            if (attribute != null) {
                gateway.setAttribute(attribute);
            }
        }
        gateway.children.addAll(launchers);
        return gateway;
    }

    // //// 声明宿主服务的包可见性 [@x380kkm 2026-09-10] ////
    private static void addHostQuery(Element manifest) {
        List<Element> queries = manifest.children.stream().filter(element -> element.name.equals("queries")).toList();
        if (queries.size() > 1) {
            throw new IllegalArgumentException("清单包含多个 queries 元素");
        }
        Element query = queries.isEmpty() ? new Element(null, "queries") : queries.get(0);
        if (queries.isEmpty()) {
            manifest.children.add(query);
        }
        if (query.children.stream().noneMatch(element -> element.name.equals("package")
            && HOST_PACKAGE.equals(element.value(ANDROID, "name")))) {
            Element host = new Element(null, "package");
            host.setAttribute(new Attribute(ANDROID, "name", NAME_ID, NodeVisitor.TYPE_STRING, HOST_PACKAGE));
            query.children.add(host);
        }
    }

    // //// 读取二进制清单的完整类型化节点树 [@x380kkm 2026-09-10] ////
    private static Document read(byte[] bytes) throws Exception {
        Document document = new Document();
        new AxmlReader(bytes).accept(new AxmlVisitor() {
            @Override public void ns(String prefix, String uri, int line) {
                if (uri == null) { throw new IllegalArgumentException("清单命名空间地址为空: prefix=" + prefix); }
                document.namespaces.add(new Namespace(prefix, uri, line));
            }
            @Override public NodeVisitor child(String namespace, String name) {
                Element element = new Element(namespace, name);
                document.roots.add(element);
                return reader(element);
            }
        });
        return document;
    }

    // //// 保留节点属性类型和文本位置 [@x380kkm 2026-09-10] ////
    private static NodeVisitor reader(Element element) {
        return new NodeVisitor() {
            @Override public void attr(String namespace, String name, int resourceId, int type, Object value) {
                element.attributes.add(new Attribute(namespace, name, resourceId, type, value));
            }
            @Override public NodeVisitor child(String namespace, String name) {
                Element child = new Element(namespace, name);
                element.children.add(child);
                return reader(child);
            }
            @Override public void line(int line) { element.line = line; }
            @Override public void text(int line, String text) {
                element.text.add(new Text(line, text));
            }
        };
    }

    // //// 用 AXMLWriter 输出原资源引用和修改后的节点 [@x380kkm 2026-09-10] ////
    private static byte[] write(Document document) throws Exception {
        AxmlWriter writer = new AxmlWriter();
        for (Namespace namespace : document.namespaces) {
            writer.ns(namespace.prefix, namespace.uri, namespace.line);
        }
        for (Element root : document.roots) {
            writeElement(writer, root);
        }
        writer.end();
        return writer.toByteArray();
    }

    // //// 输出单节点及其子元素 [@x380kkm 2026-09-10] ////
    private static void writeElement(NodeVisitor parent, Element element) {
        if (element.name == null) { throw new IllegalArgumentException("清单节点名称为空"); }
        NodeVisitor node = parent.child(element.namespace, element.name);
        node.line(element.line);
        for (Attribute attribute : element.attributes) {
            if (attribute.value == null) {
                throw new IllegalArgumentException("清单属性值为空: " + element.name + "/" + attribute.name);
            }
            node.attr(attribute.namespace, attribute.name, attribute.resourceId, attribute.type, attribute.value);
        }
        for (Text text : element.text) {
            if (text.value == null) { throw new IllegalArgumentException("清单文本为空: " + element.name); }
            node.text(text.line, text.value);
        }
        for (Element child : element.children) {
            writeElement(node, child);
        }
        node.end();
    }

    // //// 定位唯一结构节点 [@x380kkm 2026-09-10] ////
    private static Element requireSingle(List<Element> elements, String name) {
        List<Element> matches = elements.stream().filter(element -> element.name.equals(name)).toList();
        if (matches.size() != 1) {
            throw new IllegalArgumentException("清单元素数量错误: " + name + "=" + matches.size());
        }
        return matches.get(0);
    }

    private record Attribute(String namespace, String name, int resourceId, int type, Object value) {}
    private record Namespace(String prefix, String uri, int line) {}
    private record Text(int line, String value) {}

    private static final class Document {
        final List<Namespace> namespaces = new ArrayList<>();
        final List<Element> roots = new ArrayList<>();
    }

    // //// 保存类型化清单节点并按名称替换属性 [@x380kkm 2026-09-10] ////
    private static final class Element {
        final String namespace;
        final String name;
        final List<Attribute> attributes = new ArrayList<>();
        final List<Element> children = new ArrayList<>();
        final List<Text> text = new ArrayList<>();
        int line;

        Element(String namespace, String name) { this.namespace = namespace; this.name = name; }
        Attribute attribute(String namespace, String name) {
            return attributes.stream().filter(attribute -> Objects.equals(namespace, attribute.namespace)
                && name.equals(attribute.name)).findFirst().orElse(null);
        }
        Object value(String namespace, String name) {
            Attribute attribute = attribute(namespace, name);
            return attribute == null ? null : attribute.value;
        }
        void setAttribute(Attribute attribute) {
            attributes.removeIf(existing -> Objects.equals(existing.namespace, attribute.namespace)
                && existing.name.equals(attribute.name));
            attributes.add(attribute);
        }
    }
}
