// audience: internal
// # android-companion-cdn
//
// 该类读取伴随 APK 的 CDN 清单, 从安装包或用户选择的资源流解包到应用外部目录,
// 并为个人服务提供已验证的版本化目录.

package dev.starpoint.personalservice;

import android.content.Context;
import android.content.res.AssetManager;
import android.util.Log;

import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileNotFoundException;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.RandomAccessFile;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.NoSuchFileException;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

final class CdnAssetInstaller {
    static final String ASSET_ROOT = "starpoint-personal-service-cdn";
    static final String MANIFEST_NAME = "manifest.sha256";

    private static final String COMPLETE_MARKER = ".complete";
    private static final String EXTERNAL_CDN_DIRECTORY = "starpoint-personal-service/cdn";
    private static final byte[] BUNDLE_MAGIC = "SPAPKBDL".getBytes(StandardCharsets.US_ASCII);
    private static final byte[] EOCD_SIGNATURE = new byte[] {0x50, 0x4b, 0x05, 0x06};
    private static final int BUNDLE_VERSION = 1;
    private static final int BUNDLE_FLAG_DUPLICATE_EOCD = 1;
    private static final int BUNDLE_FOOTER_SIZE = 64;
    private static final int EOCD_MIN_SIZE = 22;
    private static final int EOCD_SEARCH_LIMIT = 65_557;
    private static final int COPY_BUFFER_SIZE = 1024 * 1024;
    private static final long FREE_SPACE_MARGIN = 64L * 1024L * 1024L;

    private static volatile String lastStatus = "等待个人服务 CDN";
    private static volatile boolean resourceDocumentRequired;

    private CdnAssetInstaller() {
    }

    interface StatusListener {
        void onStatusChanged(String status);
    }

    // //// 取得 CDN 安装状态 [@x380kkm 2026-08-31] ////
    static String status() {
        return lastStatus;
    }

    static boolean requiresResourceDocument() {
        return resourceDocumentRequired;
    }
    // //// /取得 CDN 安装状态 ////

    // //// 复用或安装已验证的 CDN 目录 [@x380kkm 2026-08-31] ////
    static synchronized File requireExternal(Context context, StatusListener statusListener) {
        try {
            AssetManifest manifest = readManifest(context.getAssets());
            File externalFiles = context.getExternalFilesDir(null);
            if (externalFiles == null) {
                throw new IllegalStateException("伴随应用外部数据目录不可用.");
            }
            File installRoot = new File(externalFiles, EXTERNAL_CDN_DIRECTORY);
            File destination = new File(installRoot, manifest.version);
            String installPath = installRoot.getCanonicalPath() + File.separator;
            if (!destination.getCanonicalPath().startsWith(installPath)) {
                throw new IllegalStateException("外部 CDN 路径越过安装目录.");
            }
            updateStatus(statusListener, "正在检查 CDN 资源文件");
            if (isCompleteDirectory(destination, manifest)) {
                cleanupObsoleteVersions(installRoot, manifest.version);
                resourceDocumentRequired = false;
                updateStatus(statusListener, "CDN 已就绪");
                return destination;
            }
            if (usesResourceDocument(context.getAssets())) {
                if (recoverCompleteDirectory(installRoot, destination, manifest)) {
                    cleanupObsoleteVersions(installRoot, manifest.version);
                    resourceDocumentRequired = false;
                    updateStatus(statusListener, "CDN 已就绪");
                    return destination;
                }
                resourceDocumentRequired = true;
                throw new IllegalStateException("请选择与安装包配套的 .starpoint-resources 文件.");
            }
            updateStatus(statusListener, "正在检查 CDN 可用空间");
            installBundle(
                new File(context.getApplicationInfo().sourceDir),
                manifest,
                installRoot,
                destination,
                statusListener
            );
            cleanupObsoleteVersions(installRoot, manifest.version);
            resourceDocumentRequired = false;
            updateStatus(statusListener, "CDN 已就绪");
            return destination;
        } catch (IOException error) {
            updateStatus(statusListener, readableFailure("CDN 验证失败", error));
            throw new IllegalStateException("无法验证伴随服务 CDN.", error);
        } catch (RuntimeException error) {
            updateStatus(statusListener, readableFailure("CDN 安装失败", error));
            throw error;
        }
    }
    // //// /复用或安装已验证的 CDN 目录 ////

    // //// 从用户选择的资源流安装签名清单中的文件 [@x380kkm 2026-09-26] ////
    static synchronized void importResourceDocument(Context context, InputStream source, StatusListener listener)
        throws IOException {
        AssetManifest manifest = readManifest(context.getAssets());
        File externalFiles = context.getExternalFilesDir(null);
        if (externalFiles == null) throw new IOException("伴随应用外部数据目录不可用.");
        File installRoot = new File(externalFiles, EXTERNAL_CDN_DIRECTORY);
        File destination = new File(installRoot, manifest.version);
        if (!destination.getCanonicalPath().startsWith(installRoot.getCanonicalPath() + File.separator)) {
            throw new IOException("外部 CDN 路径越过安装目录.");
        }
        installResourceStream(source, manifest, installRoot, destination, listener);
        resourceDocumentRequired = false;
        cleanupObsoleteVersions(installRoot, manifest.version);
        updateStatus(listener, "CDN 已就绪");
    }

    private static boolean usesResourceDocument(AssetManager assets) throws IOException {
        try (InputStream input = assets.open(ASSET_ROOT + "/resource-delivery.txt")) {
            String mode = new String(readAll(input), StandardCharsets.UTF_8).trim();
            if (mode.equals("external")) return true;
            if (mode.equals("embedded")) return false;
            throw new IOException("资源分发模式无效.");
        } catch (FileNotFoundException missing) {
            return false;
        }
    }

    private static void installResourceStream(InputStream source, AssetManifest manifest,
        File installRoot, File destination, StatusListener listener) throws IOException {
        byte[] bytes = new byte[BUNDLE_FOOTER_SIZE];
        new DataInputStream(source).readFully(bytes);
        ByteBuffer header = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
        byte[] magic = new byte[BUNDLE_MAGIC.length];
        header.get(magic);
        int version = header.getInt();
        int flags = header.getInt();
        long offset = header.getLong();
        long length = header.getLong();
        byte[] digest = new byte[32];
        header.get(digest);
        if (!Arrays.equals(magic, BUNDLE_MAGIC) || version != BUNDLE_VERSION || flags != 0
                || offset != BUNDLE_FOOTER_SIZE || length != manifest.totalBytes
                || !Arrays.equals(digest, digestBytes(manifest.bytes))) {
            throw new IOException("资源文件与当前安装包的清单不匹配.");
        }
        installPayload(manifest, installRoot, destination, listener, staging -> {
            extractPayload(source, manifest, staging, listener);
            if (source.read() != -1) throw new IOException("资源文件包含清单之外的数据.");
        });
    }
    // //// /从用户选择的资源流安装签名清单中的文件 ////

    // //// 读取并验证 APK 内的 CDN 清单 [@x380kkm 2026-08-31] ////
    private static AssetManifest readManifest(AssetManager assets) throws IOException {
        byte[] bytes;
        try (InputStream input = assets.open(ASSET_ROOT + "/" + MANIFEST_NAME)) {
            bytes = readAll(input);
        }
        return parseManifest(bytes);
    }

    private static AssetManifest parseManifest(byte[] bytes) {
        if (bytes.length == 0) {
            throw new IllegalStateException("内嵌 CDN 清单为空.");
        }
        List<CdnEntry> entries = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        long totalBytes = 0;
        String content = new String(bytes, StandardCharsets.UTF_8);
        for (String line : content.split("\\r?\\n", -1)) {
            if (line.isEmpty()) {
                continue;
            }
            String[] fields = line.split("\\t", 3);
            if (fields.length != 3 || !fields[0].matches("[0-9a-f]{64}")) {
                throw new IllegalStateException("内嵌 CDN 清单格式无效.");
            }
            long length;
            try {
                length = Long.parseLong(fields[1]);
            } catch (NumberFormatException error) {
                throw new IllegalStateException("内嵌 CDN 文件长度无效.", error);
            }
            if (length < 0 || !seen.add(fields[2])) {
                throw new IllegalStateException("内嵌 CDN 文件记录无效.");
            }
            validateRelativePath(fields[2]);
            if (Long.MAX_VALUE - totalBytes < length) {
                throw new IllegalStateException("内嵌 CDN 文件总长度溢出.");
            }
            totalBytes += length;
            entries.add(new CdnEntry(fields[0], length, fields[2]));
        }
        if (entries.isEmpty()) {
            throw new IllegalStateException("内嵌 CDN 清单没有有效记录.");
        }
        String digest = sha256(bytes);
        return new AssetManifest(bytes, digest, digest.substring(0, 16), entries, totalBytes);
    }
    // //// /读取并验证 APK 内的 CDN 清单 ////

    // //// 从 APK 尾随 bundle 安装 CDN [@x380kkm 2026-08-31] ////
    private static void installBundle(
        File sourceFile,
        AssetManifest manifest,
        File installRoot,
        File destination,
        StatusListener statusListener
    ) throws IOException {
        if (!sourceFile.isFile()) {
            throw new IOException("伴随 APK 文件不存在: " + sourceFile.getAbsolutePath());
        }
        BundleLocation bundle;
        try (RandomAccessFile source = new RandomAccessFile(sourceFile, "r")) {
            bundle = readBundleLocation(source);
        }
        if (!Arrays.equals(bundle.manifestDigest, digestBytes(manifest.bytes))) {
            throw new IOException("APK bundle 清单摘要不一致.");
        }
        if (bundle.payloadLength != manifest.totalBytes) {
            throw new IOException("CDN 清单长度与 payload 不一致.");
        }
        installPayload(manifest, installRoot, destination, statusListener, staging -> {
            try (FileInputStream source = new FileInputStream(sourceFile)) {
                source.getChannel().position(bundle.payloadOffset);
                extractPayload(source, manifest, staging, statusListener);
            }
        });
    }

    private interface PayloadExtractor {
        void extract(File staging) throws IOException;
    }

    private static void installPayload(AssetManifest manifest, File installRoot, File destination,
        StatusListener statusListener, PayloadExtractor extractor) throws IOException {
        if (!installRoot.isDirectory() && !installRoot.mkdirs()) {
            throw new IOException("无法创建 CDN 安装目录.");
        }
        if (recoverCompleteDirectory(installRoot, destination, manifest)) {
            return;
        }
        File staging = new File(installRoot, "." + manifest.version + ".staging");
        deleteRecursively(staging);
        deleteRecursively(new File(installRoot, "." + manifest.version + ".previous"));
        long requiredBytes = requiredBytes(manifest);
        long usableBytes = installRoot.getUsableSpace();
        if (usableBytes < requiredBytes) {
            throw new IOException(
                "可用空间不足: required=" + requiredBytes + " available=" + usableBytes
            );
        }
        if (!staging.mkdirs()) {
            throw new IOException("无法创建 CDN 临时目录.");
        }
        try {
            updateStatus(statusListener, "正在解包 CDN 资源");
            extractor.extract(staging);
            writeFileAtomically(staging, MANIFEST_NAME, manifest.bytes);
            writeFileAtomically(
                staging,
                COMPLETE_MARKER,
                (manifest.digest + "\n").getBytes(StandardCharsets.UTF_8)
            );
            replaceDirectory(staging, destination, installRoot, manifest.version);
        } catch (IOException | RuntimeException error) {
            try {
                deleteRecursively(staging);
            } catch (IOException cleanupError) {
                error.addSuppressed(cleanupError);
            }
            throw error;
        }
    }

    private static long requiredBytes(AssetManifest manifest) throws IOException {
        long required = manifest.totalBytes;
        required = checkedAdd(required, manifest.bytes.length);
        required = checkedAdd(required, manifest.digest.length() + 1L);
        return checkedAdd(required, FREE_SPACE_MARGIN);
    }

    private static long checkedAdd(long left, long right) throws IOException {
        if (right < 0 || Long.MAX_VALUE - left < right) {
            throw new IOException("CDN 所需空间溢出.");
        }
        return left + right;
    }

    private static void extractPayload(
        InputStream source,
        AssetManifest manifest,
        File staging,
        StatusListener statusListener
    ) throws IOException {
        byte[] buffer = new byte[COPY_BUFFER_SIZE];
        for (int index = 0; index < manifest.entries.size(); index++) {
            CdnEntry entry = manifest.entries.get(index);
            File target = resolveStagingFile(staging, entry.relativePath);
            File parent = target.getParentFile();
            if (!parent.isDirectory() && !parent.mkdirs()) {
                throw new IOException("无法创建 CDN 文件目录: " + parent);
            }
            MessageDigest digest = newDigest();
            long remaining = entry.length;
            try (FileOutputStream output = new FileOutputStream(target)) {
                while (remaining > 0) {
                    int requested = (int) Math.min(buffer.length, remaining);
                    int count = source.read(buffer, 0, requested);
                    if (count <= 0) {
                        throw new IOException("CDN payload 提前结束: " + entry.relativePath);
                    }
                    output.write(buffer, 0, count);
                    digest.update(buffer, 0, count);
                    remaining -= count;
                }
            }
            String actualDigest = hex(digest.digest());
            if (!entry.digest.equals(actualDigest)) {
                throw new IOException(
                    "CDN 文件摘要不一致: " + entry.relativePath
                        + " expected=" + entry.digest + " actual=" + actualDigest
                );
            }
            int completed = index + 1;
            if (completed == manifest.entries.size() || completed % 64 == 0) {
                updateStatus(statusListener,
                    "正在解包 CDN 资源 " + completed + "/" + manifest.entries.size());
            }
        }
    }

    private static File resolveStagingFile(File staging, String relativePath) throws IOException {
        File target = new File(staging, relativePath.replace('/', File.separatorChar));
        String root = staging.getCanonicalPath() + File.separator;
        if (!target.getCanonicalPath().startsWith(root)) {
            throw new IOException("CDN 路径越过临时目录: " + relativePath);
        }
        return target;
    }

    private static void writeFileAtomically(
        File directory,
        String relativePath,
        byte[] bytes
    ) throws IOException {
        File target = resolveStagingFile(directory, relativePath);
        File temporary = new File(target.getPath() + ".tmp");
        try (FileOutputStream output = new FileOutputStream(temporary)) {
            output.write(bytes);
            output.flush();
            output.getFD().sync();
        }
        try {
            Files.move(
                temporary.toPath(),
                target.toPath(),
                StandardCopyOption.ATOMIC_MOVE,
                StandardCopyOption.REPLACE_EXISTING
            );
        } catch (AtomicMoveNotSupportedException | UnsupportedOperationException error) {
            if (!temporary.renameTo(target)) {
                throw new IOException("无法原子写入 CDN 标记: " + target.getAbsolutePath(), error);
            }
        } finally {
            if (temporary.exists() && !temporary.delete()) {
                throw new IOException("无法清理 CDN 临时标记: " + temporary.getAbsolutePath());
            }
        }
    }

    private static void replaceDirectory(
        File staging,
        File destination,
        File installRoot,
        String version
    ) throws IOException {
        File previous = new File(installRoot, "." + version + ".previous");
        deleteRecursively(previous);
        if (destination.exists() && !destination.renameTo(previous)) {
            throw new IOException("无法保留现有 CDN 目录.");
        }
        boolean moved = false;
        try {
            moved = moveAtomically(staging, destination);
            if (!moved) {
                throw new IOException("无法原子替换 CDN 目录.");
            }
        } finally {
            if (moved) {
                deleteRecursively(previous);
            } else if (previous.exists() && !previous.renameTo(destination)) {
                throw new IOException("无法恢复现有 CDN 目录.");
            }
        }
    }

    private static boolean moveAtomically(File source, File destination) throws IOException {
        try {
            Files.move(
                source.toPath(),
                destination.toPath(),
                StandardCopyOption.ATOMIC_MOVE,
                StandardCopyOption.REPLACE_EXISTING
            );
            return true;
        } catch (AtomicMoveNotSupportedException | UnsupportedOperationException error) {
            return source.renameTo(destination);
        }
    }
    // //// /从 APK 尾随 bundle 安装 CDN ////

    // //// 恢复已完成目录并回收失去引用的 CDN 版本 [@x380kkm 2026-09-26] ////
    private static boolean recoverCompleteDirectory(
        File installRoot,
        File destination,
        AssetManifest manifest
    ) throws IOException {
        File staging = new File(installRoot, "." + manifest.version + ".staging");
        if (isCompleteDirectory(staging, manifest)) {
            replaceDirectory(staging, destination, installRoot, manifest.version);
            return true;
        }
        File previous = new File(installRoot, "." + manifest.version + ".previous");
        if (isCompleteDirectory(previous, manifest)) {
            deleteRecursively(destination);
            if (!moveAtomically(previous, destination)) {
                throw new IOException("无法恢复已完成的 CDN 目录.");
            }
            return true;
        }
        return false;
    }

    private static boolean isCompleteDirectory(File directory, AssetManifest manifest) throws IOException {
        if (Files.isSymbolicLink(directory.toPath())
            || !hasMatchingManifest(directory, manifest.digest)
            || !hasCompleteMarker(directory, manifest.digest)) {
            return false;
        }
        for (CdnEntry entry : manifest.entries) {
            File target = new File(directory, entry.relativePath);
            try {
                BasicFileAttributes attributes = Files.readAttributes(
                    target.toPath(), BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
                if (!attributes.isRegularFile() || attributes.size() != entry.length) {
                    return false;
                }
            } catch (NoSuchFileException error) {
                return false;
            }
        }
        return true;
    }

    private static void cleanupObsoleteVersions(File installRoot, String activeVersion) {
        File[] directories = installRoot.listFiles();
        if (directories == null) {
            return;
        }
        for (File directory : directories) {
            String name = directory.getName();
            if (name.equals(activeVersion) || Files.isSymbolicLink(directory.toPath())) {
                continue;
            }
            try {
                boolean temporary = name.matches("\\.[0-9a-f]{16}\\.(staging|previous)");
                if (temporary || isOwnedVersionDirectory(directory)) {
                    deleteRecursively(directory);
                }
            } catch (IOException error) {
                Log.w("StarpointPersonalService", "无法清理旧 CDN 目录: " + name, error);
            }
        }
    }

    private static boolean isOwnedVersionDirectory(File directory) throws IOException {
        if (!directory.getName().matches("[0-9a-f]{16}")) {
            return false;
        }
        File manifest = new File(directory, MANIFEST_NAME);
        if (!manifest.isFile()) {
            return false;
        }
        String digest;
        try (InputStream input = new FileInputStream(manifest)) {
            digest = sha256(readAll(input));
        }
        return digest.startsWith(directory.getName()) && hasCompleteMarker(directory, digest);
    }
    // //// /恢复已完成目录并回收失去引用的 CDN 版本 ////

    // //// 读取尾随 footer 并核对重复 EOCD [@x380kkm 2026-08-31] ////
    private static BundleLocation readBundleLocation(RandomAccessFile source) throws IOException {
        long fileLength = source.length();
        Eocd finalEocd = findEocd(source, 0, fileLength);
        long footerOffset = finalEocd.offset - BUNDLE_FOOTER_SIZE;
        if (footerOffset < 0) {
            throw new IOException("APK bundle 缺少 footer.");
        }
        byte[] footerBytes = readAt(source, footerOffset, BUNDLE_FOOTER_SIZE);
        ByteBuffer footer = ByteBuffer.wrap(footerBytes).order(ByteOrder.LITTLE_ENDIAN);
        byte[] magic = new byte[BUNDLE_MAGIC.length];
        footer.get(magic);
        int version = footer.getInt();
        int flags = footer.getInt();
        long payloadOffset = footer.getLong();
        long payloadLength = footer.getLong();
        byte[] manifestDigest = new byte[32];
        footer.get(manifestDigest);
        if (
            !Arrays.equals(magic, BUNDLE_MAGIC)
                || version != BUNDLE_VERSION
                || (flags & BUNDLE_FLAG_DUPLICATE_EOCD) == 0
        ) {
            throw new IOException("APK bundle footer 标识无效.");
        }
        if (
            payloadOffset < 0
                || payloadLength < 0
                || payloadOffset > footerOffset
                || payloadLength > footerOffset - payloadOffset
                || payloadOffset + payloadLength != footerOffset
        ) {
            throw new IOException("APK bundle payload 范围无效.");
        }
        Eocd baseEocd = findEocd(source, 0, payloadOffset);
        if (!Arrays.equals(baseEocd.bytes, finalEocd.bytes)) {
            throw new IOException("APK bundle 重复 EOCD 不一致.");
        }
        if (payloadOffset != baseEocd.offset + baseEocd.bytes.length) {
            throw new IOException("APK bundle payload 没有紧接基础 APK.");
        }
        return new BundleLocation(payloadOffset, payloadLength, manifestDigest);
    }

    private static Eocd findEocd(RandomAccessFile source, long rangeStart, long rangeEnd)
        throws IOException {
        if (rangeEnd - rangeStart < EOCD_MIN_SIZE) {
            throw new IOException("APK 缺少 ZIP 末尾记录.");
        }
        int readSize = (int) Math.min(rangeEnd - rangeStart, EOCD_SEARCH_LIMIT);
        byte[] tail = new byte[readSize];
        source.seek(rangeEnd - readSize);
        source.readFully(tail);
        for (int cursor = readSize - EOCD_MIN_SIZE; cursor >= 0; cursor--) {
            if (!matches(tail, cursor, EOCD_SIGNATURE)) {
                continue;
            }
            int commentLength = unsignedShort(tail, cursor + 20);
            int end = cursor + EOCD_MIN_SIZE + commentLength;
            if (end != readSize) {
                continue;
            }
            int disk = unsignedShort(tail, cursor + 4);
            int centralDisk = unsignedShort(tail, cursor + 6);
            int diskEntries = unsignedShort(tail, cursor + 8);
            int totalEntries = unsignedShort(tail, cursor + 10);
            long centralSize = unsignedInt(tail, cursor + 12);
            long centralOffset = unsignedInt(tail, cursor + 16);
            if (
                disk != 0
                    || centralDisk != 0
                    || diskEntries != totalEntries
                    || diskEntries == 0xffff
                    || centralSize == 0xffffffffL
                    || centralOffset == 0xffffffffL
            ) {
                continue;
            }
            long offset = rangeEnd - readSize + cursor;
            if (centralOffset + centralSize > offset) {
                continue;
            }
            return new Eocd(offset, Arrays.copyOfRange(tail, cursor, end));
        }
        throw new IOException("APK 缺少有效 ZIP 末尾记录.");
    }

    private static byte[] readAt(RandomAccessFile source, long offset, int length) throws IOException {
        byte[] bytes = new byte[length];
        source.seek(offset);
        source.readFully(bytes);
        return bytes;
    }

    private static boolean matches(byte[] bytes, int offset, byte[] expected) {
        if (offset < 0 || offset + expected.length > bytes.length) {
            return false;
        }
        for (int index = 0; index < expected.length; index++) {
            if (bytes[offset + index] != expected[index]) {
                return false;
            }
        }
        return true;
    }

    private static int unsignedShort(byte[] bytes, int offset) {
        return (bytes[offset] & 0xff) | ((bytes[offset + 1] & 0xff) << 8);
    }

    private static long unsignedInt(byte[] bytes, int offset) {
        return unsignedShort(bytes, offset) | ((long) unsignedShort(bytes, offset + 2) << 16);
    }
    // //// /读取尾随 footer 并核对重复 EOCD ////

    // //// 核验清单路径和已安装目录 [@x380kkm 2026-08-31] ////
    private static boolean hasMatchingManifest(File directory, String expected) throws IOException {
        File manifest = new File(directory, MANIFEST_NAME);
        if (!manifest.isFile()) {
            return false;
        }
        try (InputStream input = new FileInputStream(manifest)) {
            return sha256(readAll(input)).equals(expected);
        }
    }

    private static boolean hasCompleteMarker(File directory, String expected) throws IOException {
        File marker = new File(directory, COMPLETE_MARKER);
        if (!directory.isDirectory() || !marker.isFile()) {
            return false;
        }
        try (InputStream input = new FileInputStream(marker)) {
            return new String(readAll(input), StandardCharsets.UTF_8).equals(expected + "\n");
        }
    }

    private static void validateRelativePath(String relativePath) {
        if (
            relativePath.isEmpty()
                || relativePath.startsWith("/")
                || relativePath.startsWith("\\")
                || relativePath.contains("\\")
                || relativePath.contains("\t")
                || relativePath.contains("\r")
                || relativePath.contains("\n")
                || relativePath.indexOf('\0') >= 0
        ) {
            throw new IllegalStateException("内嵌 CDN 路径无效.");
        }
        for (String segment : relativePath.split("/", -1)) {
            if (segment.isEmpty() || segment.equals(".") || segment.equals("..")) {
                throw new IllegalStateException("内嵌 CDN 路径无效.");
            }
        }
    }
    // //// /核验清单路径和已安装目录 ////

    // //// 读取字节并计算摘要 [@x380kkm 2026-08-31] ////
    private static byte[] readAll(InputStream input) throws IOException {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        int count;
        while ((count = input.read(buffer)) != -1) {
            output.write(buffer, 0, count);
        }
        return output.toByteArray();
    }

    private static String sha256(byte[] bytes) {
        return hex(digestBytes(bytes));
    }

    private static byte[] digestBytes(byte[] bytes) {
        MessageDigest digest = newDigest();
        return digest.digest(bytes);
    }

    private static MessageDigest newDigest() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("Android 缺少 SHA-256.", error);
        }
    }

    private static String hex(byte[] bytes) {
        StringBuilder result = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            result.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        }
        return result.toString();
    }

    private static String readableFailure(String prefix, Throwable error) {
        String message = error.getMessage();
        return message == null || message.trim().isEmpty() ? prefix : prefix + ": " + message;
    }

    private static void updateStatus(StatusListener listener, String status) {
        lastStatus = status;
        if (listener != null) {
            listener.onStatusChanged(status);
        }
    }

    private static void deleteRecursively(File path) throws IOException {
        if (Files.isSymbolicLink(path.toPath())) {
            Files.deleteIfExists(path.toPath());
            return;
        }
        if (!path.exists()) {
            return;
        }
        File[] children = path.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteRecursively(child);
            }
        }
        if (!path.delete() && path.exists()) {
            throw new IOException("无法清理 CDN 临时项: " + path.getAbsolutePath());
        }
    }
    // //// /读取字节并计算摘要 ////

    private static final class AssetManifest {
        private final byte[] bytes;
        private final String digest;
        private final String version;
        private final List<CdnEntry> entries;
        private final long totalBytes;

        private AssetManifest(
            byte[] bytes,
            String digest,
            String version,
            List<CdnEntry> entries,
            long totalBytes
        ) {
            this.bytes = bytes;
            this.digest = digest;
            this.version = version;
            this.entries = entries;
            this.totalBytes = totalBytes;
        }
    }

    private static final class CdnEntry {
        private final String digest;
        private final long length;
        private final String relativePath;

        private CdnEntry(String digest, long length, String relativePath) {
            this.digest = digest;
            this.length = length;
            this.relativePath = relativePath;
        }
    }

    private static final class Eocd {
        private final long offset;
        private final byte[] bytes;

        private Eocd(long offset, byte[] bytes) {
            this.offset = offset;
            this.bytes = bytes;
        }
    }

    private static final class BundleLocation {
        private final long payloadOffset;
        private final long payloadLength;
        private final byte[] manifestDigest;

        private BundleLocation(long payloadOffset, long payloadLength, byte[] manifestDigest) {
            this.payloadOffset = payloadOffset;
            this.payloadLength = payloadLength;
            this.manifestDigest = manifestDigest;
        }
    }
}
