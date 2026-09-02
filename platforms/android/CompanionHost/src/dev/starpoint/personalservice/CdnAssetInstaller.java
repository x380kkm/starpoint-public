// audience: internal
// # android-companion-cdn
//
// 该类读取伴随 APK 的 CDN 清单, 从尾随 bundle 流式解包资源到应用外部目录,
// 并为个人服务提供已验证的版本化目录.

package dev.starpoint.personalservice;

import android.content.Context;
import android.content.res.AssetManager;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.RandomAccessFile;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
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

    private CdnAssetInstaller() {
    }

    interface StatusListener {
        void onStatusChanged(String status);
    }

    // //// 取得 CDN 安装状态 [@x380kkm 2026-08-31] ////
    static String status() {
        return lastStatus;
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
            if (
                hasMatchingManifest(destination, manifest.digest)
                    && hasCompleteMarker(destination, manifest.digest)
            ) {
                updateStatus(statusListener, "CDN 已就绪");
                return destination;
            }
            updateStatus(statusListener, "正在检查 CDN 可用空间");
            installBundle(
                new File(context.getApplicationInfo().sourceDir),
                manifest,
                installRoot,
                destination,
                statusListener
            );
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
        if (!installRoot.isDirectory() && !installRoot.mkdirs()) {
            throw new IOException("无法创建 CDN 安装目录.");
        }
        long requiredBytes = requiredBytes(manifest);
        long usableBytes = installRoot.getUsableSpace();
        if (usableBytes < requiredBytes) {
            throw new IOException(
                "可用空间不足: required=" + requiredBytes + " available=" + usableBytes
            );
        }

        File staging = new File(installRoot, "." + manifest.version + ".staging");
        deleteRecursively(staging);
        if (!staging.mkdirs()) {
            throw new IOException("无法创建 CDN 临时目录.");
        }
        try {
            updateStatus(statusListener, "正在解包 CDN 资源");
            extractPayload(sourceFile, bundle, manifest, staging, statusListener);
            writeFile(staging, MANIFEST_NAME, manifest.bytes);
            writeFile(
                staging,
                COMPLETE_MARKER,
                (manifest.digest + "\n").getBytes(StandardCharsets.UTF_8)
            );
            replaceDirectory(staging, destination, installRoot, manifest.version);
        } catch (IOException | RuntimeException error) {
            deleteRecursively(staging);
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
        File sourceFile,
        BundleLocation bundle,
        AssetManifest manifest,
        File staging,
        StatusListener statusListener
    ) throws IOException {
        long payloadEnd = checkedAdd(bundle.payloadOffset, bundle.payloadLength);
        try (RandomAccessFile source = new RandomAccessFile(sourceFile, "r")) {
            source.seek(bundle.payloadOffset);
            byte[] buffer = new byte[COPY_BUFFER_SIZE];
            for (int index = 0; index < manifest.entries.size(); index++) {
                CdnEntry entry = manifest.entries.get(index);
                if (entry.length > payloadEnd - source.getFilePointer()) {
                    throw new IOException("CDN payload 长度不足: " + entry.relativePath);
                }
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
                        if (count < 0) {
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
                    updateStatus(
                        statusListener,
                        "正在解包 CDN 资源 " + completed + "/" + manifest.entries.size()
                    );
                }
            }
            if (source.getFilePointer() != payloadEnd) {
                throw new IOException("CDN 清单长度与 payload 不一致.");
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

    private static void writeFile(File directory, String relativePath, byte[] bytes) throws IOException {
        File target = resolveStagingFile(directory, relativePath);
        try (FileOutputStream output = new FileOutputStream(target)) {
            output.write(bytes);
            output.flush();
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
