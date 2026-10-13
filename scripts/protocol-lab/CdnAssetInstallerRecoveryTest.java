// audience: internal
// # android-cdn-recovery-test
// 该回放在主机临时目录中调用 CDN 安装器, Android API 类型来自 SDK 编译包.

package dev.starpoint.personalservice;

import java.io.ByteArrayOutputStream;
import java.io.ByteArrayInputStream;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Comparator;
import java.util.zip.ZipOutputStream;

public final class CdnAssetInstallerRecoveryTest {
    private static final Class<?> MANIFEST_TYPE;

    static {
        try {
            MANIFEST_TYPE = Class.forName(
                "dev.starpoint.personalservice.CdnAssetInstaller$AssetManifest"
            );
        } catch (ClassNotFoundException error) {
            throw new ExceptionInInitializerError(error);
        }
    }

    // //// 回放空间不足和目录提交中断后的恢复 [@x380kkm 2026-09-10] ////
    public static void main(String[] arguments) throws Exception {
        Path directory = Files.createTempDirectory("starpoint-cdn-recovery-");
        try {
            removesPartialStagingBeforeSpaceCheck(directory.resolve("partial"));
            recoversCompleteStagingWithoutExtraSpace(directory.resolve("complete"));
            recoversPreviousDirectoryWithoutExtraSpace(directory.resolve("previous"));
            rejectsIncompleteInstalledResources(directory.resolve("missing-installed"));
            restoresPreviousWhenStagingResourcesAreMissing(directory.resolve("missing-staging"));
            repairsTruncatedStagingFromResourceDocument(directory.resolve("truncated-staging"));
            keepsActiveResourcesAndUnrelatedFiles(directory.resolve("cleanup"));
            importsSequentialResourceDocument(directory.resolve("document"));
            rejectsResourceDocumentWithoutReplacingCurrentFiles(directory.resolve("invalid-document"));
            keepsResourcesWhenDocumentHasInsufficientSpace(directory.resolve("document-space"));
            System.out.println("CDN 目录恢复, 顺序资源导入及无效文件/空间不足保护回放通过.");
        } finally {
            try (java.util.stream.Stream<Path> paths = Files.walk(directory)) {
                for (Path path : (Iterable<Path>) paths.sorted(Comparator.reverseOrder())::iterator) {
                    Files.delete(path);
                }
            }
        }
    }

    private static void removesPartialStagingBeforeSpaceCheck(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path staging = root.resolve("." + bundle.version + ".staging");
        Files.createDirectories(staging);
        Files.write(staging.resolve("partial.data"), new byte[] {1});
        File installRoot = new File(root.toString()) {
            @Override
            public long getUsableSpace() {
                return Files.exists(staging) ? 0 : 1024L * 1024L * 1024L;
            }
        };
        install(bundle, installRoot);
        require(
            Arrays.equals(Files.readAllBytes(root.resolve(bundle.version).resolve("sample.bin")), bundle.payload),
            "残留临时目录清理后必须完成资源安装."
        );
    }

    private static void recoversCompleteStagingWithoutExtraSpace(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path staging = root.resolve("." + bundle.version + ".staging");
        writeCompleteDirectory(staging, bundle.manifestBytes, bundle.payload);
        install(bundle, noFreeSpace(root));
        require(Files.isRegularFile(root.resolve(bundle.version).resolve("sample.bin")), "完整临时目录必须恢复.");
        require(!Files.exists(staging), "提交后的临时目录必须消失.");
    }

    private static void recoversPreviousDirectoryWithoutExtraSpace(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path previous = root.resolve("." + bundle.version + ".previous");
        writeCompleteDirectory(previous, bundle.manifestBytes, bundle.payload);
        install(bundle, noFreeSpace(root));
        require(Files.isRegularFile(root.resolve(bundle.version).resolve("sample.bin")), "完整保留目录必须恢复.");
        require(!Files.exists(previous), "恢复后的保留目录必须消失.");
    }

    private static void keepsActiveResourcesAndUnrelatedFiles(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path active = root.resolve(bundle.version);
        writeCompleteDirectory(active, bundle.manifestBytes, bundle.payload);
        byte[] oldManifest = "old manifest".getBytes(StandardCharsets.UTF_8);
        Path obsolete = root.resolve(digest(oldManifest).substring(0, 16));
        writeCompleteDirectory(obsolete, oldManifest, new byte[] {2});
        Path unknown = root.resolve("0123456789abcdef");
        Files.createDirectories(unknown);
        Files.write(unknown.resolve("owned-by-user"), new byte[] {3});
        Path save = root.resolveSibling("player-save.sqlite");
        Files.write(save, new byte[] {4});
        Path incomplete = root.resolve(".fedcba9876543210.staging");
        Files.createDirectories(incomplete);
        Files.write(incomplete.resolve("partial.bin"), new byte[] {5});
        invoke(
            "cleanupObsoleteVersions",
            new Class<?>[] {File.class, String.class},
            root.toFile(),
            bundle.version
        );
        require(Files.isRegularFile(active.resolve("sample.bin")), "当前资源必须保留.");
        require(!Files.exists(obsolete), "失去引用的已安装 CDN 必须清理.");
        require(!Files.exists(incomplete), "失去引用的解包临时目录必须清理.");
        require(Files.exists(unknown.resolve("owned-by-user")), "没有 CDN 身份的目录必须保留.");
        require(Files.readAllBytes(save)[0] == 4, "CDN 目录之外的玩家存档必须保持原样.");
    }
    // //// /回放空间不足和目录提交中断后的恢复 ////

    // //// 回放资源文件缺失和截断后的目录恢复 [@x380kkm 2026-09-26] ////
    private static void rejectsIncompleteInstalledResources(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path destination = root.resolve(bundle.version);
        writeCompleteDirectory(destination, bundle.manifestBytes, bundle.payload);
        require(isCompleteDirectory(destination, bundle), "完整资源目录必须可以复用.");
        Files.write(destination.resolve("sample.bin"), new byte[0]);
        require(!isCompleteDirectory(destination, bundle), "资源被截断时必须重新导入.");
        Files.delete(destination.resolve("sample.bin"));
        require(!isCompleteDirectory(destination, bundle), "资源缺失时必须重新导入.");
    }

    private static boolean isCompleteDirectory(Path directory, BundleFixture bundle) throws Exception {
        return (Boolean) invoke("isCompleteDirectory", new Class<?>[] {File.class, MANIFEST_TYPE},
            directory.toFile(), bundle.manifest);
    }

    private static void restoresPreviousWhenStagingResourcesAreMissing(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path staging = root.resolve("." + bundle.version + ".staging");
        writeCompleteDirectory(staging, bundle.manifestBytes, bundle.payload);
        Files.delete(staging.resolve("sample.bin"));
        Path previous = root.resolve("." + bundle.version + ".previous");
        writeCompleteDirectory(previous, bundle.manifestBytes, bundle.payload);
        install(bundle, noFreeSpace(root));
        require(Arrays.equals(Files.readAllBytes(root.resolve(bundle.version).resolve("sample.bin")), bundle.payload),
            "临时目录缺少资源时必须恢复完整的保留目录.");
    }

    private static void repairsTruncatedStagingFromResourceDocument(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path staging = root.resolve("." + bundle.version + ".staging");
        writeCompleteDirectory(staging, bundle.manifestBytes, bundle.payload);
        Files.write(staging.resolve("sample.bin"), new byte[0]);
        installDocument(new ByteArrayInputStream(resourceDocument(bundle)), bundle, root.toFile());
        require(Arrays.equals(Files.readAllBytes(root.resolve(bundle.version).resolve("sample.bin")), bundle.payload),
            "临时资源被截断时必须从资源文档重新安装.");
    }
    // //// /回放资源文件缺失和截断后的目录恢复 ////

    // //// 回放资源文档导入对已安装文件的保护 [@x380kkm 2026-09-26] ////
    private static void importsSequentialResourceDocument(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        try (InputStream source = new ByteArrayInputStream(resourceDocument(bundle)) {
            @Override
            public long skip(long count) { throw new AssertionError("文档导入应顺序读取."); }
        }) {
            installDocument(source, bundle, root.toFile());
        }
        require(Arrays.equals(Files.readAllBytes(root.resolve(bundle.version).resolve("sample.bin")), bundle.payload),
            "顺序资源文档必须完整导入.");
    }

    private static void rejectsResourceDocumentWithoutReplacingCurrentFiles(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path destination = root.resolve(bundle.version);
        writeCompleteDirectory(destination, bundle.manifestBytes, bundle.payload);
        byte[] valid = resourceDocument(bundle);
        byte[] changedManifest = valid.clone();
        changedManifest[32] ^= 1;
        byte[] corrupted = valid.clone();
        corrupted[64] ^= 1;
        byte[][] invalidDocuments = {changedManifest, corrupted, Arrays.copyOf(valid, valid.length - 1),
            Arrays.copyOf(valid, valid.length + 1)};
        for (byte[] invalid : invalidDocuments) {
            try {
                installDocument(new ByteArrayInputStream(invalid), bundle, root.toFile());
                throw new AssertionError("无效或中断的资源文档必须拒绝提交.");
            } catch (IOException expected) {
                require(Arrays.equals(Files.readAllBytes(destination.resolve("sample.bin")), bundle.payload),
                    "导入失败必须保留已安装资源.");
                require(!Files.exists(root.resolve("." + bundle.version + ".staging")),
                    "失败的资源文档必须清理未提交文件.");
            }
        }
    }

    private static void keepsResourcesWhenDocumentHasInsufficientSpace(Path root) throws Exception {
        BundleFixture bundle = createBundle(root);
        Path destination = root.resolve(bundle.version);
        writeCompleteDirectory(destination, bundle.manifestBytes, bundle.payload);
        try {
            installDocument(new ByteArrayInputStream(resourceDocument(bundle)), bundle, noFreeSpace(root));
            throw new AssertionError("空间不足必须停止资源导入.");
        } catch (IOException expected) {
            require(Arrays.equals(Files.readAllBytes(destination.resolve("sample.bin")), bundle.payload),
                "空间不足必须保留已安装资源.");
        }
    }

    private static byte[] resourceDocument(BundleFixture bundle) throws Exception {
        ByteBuffer document = ByteBuffer.allocate(64 + bundle.payload.length).order(ByteOrder.LITTLE_ENDIAN);
        document.put("SPAPKBDL".getBytes(StandardCharsets.US_ASCII));
        document.putInt(1).putInt(0).putLong(64).putLong(bundle.payload.length);
        document.put(MessageDigest.getInstance("SHA-256").digest(bundle.manifestBytes));
        document.put(bundle.payload);
        return document.array();
    }

    private static void installDocument(InputStream source, BundleFixture bundle, File root) throws Exception {
        invoke("installResourceStream",
            new Class<?>[] {InputStream.class, MANIFEST_TYPE, File.class, File.class, CdnAssetInstaller.StatusListener.class},
            source, bundle.manifest, root, bundle.root.resolve(bundle.version).toFile(), null);
    }
    // //// /回放资源文档导入对已安装文件的保护 ////

    // //// 构造带尾随资源的最小安装包 [@x380kkm 2026-09-10] ////
    private static BundleFixture createBundle(Path root) throws Exception {
        Files.createDirectories(root);
        byte[] payload = "local CDN content".getBytes(StandardCharsets.UTF_8);
        byte[] manifest = (digest(payload) + "\t" + payload.length + "\tsample.bin\n")
            .getBytes(StandardCharsets.UTF_8);
        Object parsed = invoke("parseManifest", new Class<?>[] {byte[].class}, manifest);
        ByteArrayOutputStream zipBytes = new ByteArrayOutputStream();
        try (ZipOutputStream zip = new ZipOutputStream(zipBytes)) {
            zip.finish();
        }
        byte[] base = zipBytes.toByteArray();
        ByteBuffer footer = ByteBuffer.allocate(64).order(ByteOrder.LITTLE_ENDIAN);
        footer.put("SPAPKBDL".getBytes(StandardCharsets.US_ASCII));
        footer.putInt(1).putInt(1).putLong(base.length).putLong(payload.length);
        footer.put(MessageDigest.getInstance("SHA-256").digest(manifest));
        ByteArrayOutputStream complete = new ByteArrayOutputStream();
        complete.write(base);
        complete.write(payload);
        complete.write(footer.array());
        complete.write(base);
        Path apk = root.resolve("fixture.apk");
        Files.write(apk, complete.toByteArray());
        return new BundleFixture(root, apk, parsed, manifest, payload, digest(manifest).substring(0, 16));
    }

    private static void writeCompleteDirectory(Path root, byte[] manifest, byte[] payload)
        throws Exception {
        Files.createDirectories(root);
        Files.write(root.resolve("manifest.sha256"), manifest);
        Files.write(root.resolve(".complete"), (digest(manifest) + "\n").getBytes(StandardCharsets.UTF_8));
        Files.write(root.resolve("sample.bin"), payload);
    }

    private static File noFreeSpace(Path root) {
        return new File(root.toString()) {
            @Override
            public long getUsableSpace() {
                return 0;
            }
        };
    }

    private static void install(BundleFixture bundle, File root) throws Exception {
        invoke(
            "installBundle",
            new Class<?>[] {File.class, MANIFEST_TYPE, File.class, File.class, CdnAssetInstaller.StatusListener.class},
            bundle.apk.toFile(), bundle.manifest, root, bundle.root.resolve(bundle.version).toFile(), null
        );
    }

    private static Object invoke(String name, Class<?>[] types, Object... arguments) throws Exception {
        Method method = CdnAssetInstaller.class.getDeclaredMethod(name, types);
        method.setAccessible(true);
        try {
            return method.invoke(null, arguments);
        } catch (InvocationTargetException error) {
            if (error.getCause() instanceof Exception) {
                throw (Exception) error.getCause();
            }
            throw error;
        }
    }

    private static String digest(byte[] bytes) throws Exception {
        StringBuilder result = new StringBuilder();
        for (byte value : MessageDigest.getInstance("SHA-256").digest(bytes)) {
            result.append(String.format("%02x", value & 0xff));
        }
        return result.toString();
    }

    private static void require(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError(message);
        }
    }
    // //// /构造带尾随资源的最小安装包 ////

    private static final class BundleFixture {
        final Path root;
        final Path apk;
        final Object manifest;
        final byte[] manifestBytes;
        final byte[] payload;
        final String version;

        BundleFixture(Path root, Path apk, Object manifest, byte[] manifestBytes, byte[] payload, String version) {
            this.root = root;
            this.apk = apk;
            this.manifest = manifest;
            this.manifestBytes = manifestBytes;
            this.payload = payload;
            this.version = version;
        }
    }
}
