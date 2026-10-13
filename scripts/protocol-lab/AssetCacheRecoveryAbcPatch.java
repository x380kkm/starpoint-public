// audience: internal
// # asset-cache-recovery-abc-patch
// 下载缓存的空文件通过平台清单补取原始内容. 素材检查同时比较清单长度.
// 恢复任务先清除重试记录再通知父任务完成, 资源释放保留未完成的恢复集合.
// Java 17 和 FFDec 24 读取 Android 1.8.1 方法体, 输出独立的 SWF 副本.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.AVM2Code;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.instructions.InstructionDefinition;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Namespace;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import com.jpexs.decompiler.flash.tags.Tag;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class AssetCacheRecoveryAbcPatch {
    static final String READER = "pinball.asset.file.FileReader";
    static final String LOADER = "pinball.asset.file._FileReader.IntermediateDownloadFilesLoader";
    static final String CHECKER = "pinball.loading.assetDownload.check.AssetSufficiencyChecking";
    static final String RECOVERER = "pinball.loading.assetDownload.recovery.AssetRecovering";
    static final String MANIFEST_PATH = "by-path/10939-android_medium.csv/";
    static final String URL_MARKER = "/entities/" + MANIFEST_PATH;
    static final Map<String, Set<String>> METHODS = Map.of(
            READER, Set.of("fileExistsSync", "failedReadAssetFileHandler"),
            LOADER, Set.of("getDownloadedContentFile", "startDownloadFile", "openCachedFile", "successHandler", "contentFileReadComplete"),
            CHECKER, Set.of("update"),
            RECOVERER, Set.of("run", "dispose", "completeHandler"));

    private AssetCacheRecoveryAbcPatch() { }

    // //// 保存并检查通用缓存补救方法 [@x380kkm 2026-09-27] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) { throw new IllegalArgumentException("usage: AssetCacheRecoveryAbcPatch <reference.swf> <input.swf> <output.swf> <evidence.json>"); }
        Path output = Path.of(args[2]).toAbsolutePath().normalize();
        if (Files.exists(output) || output.equals(Path.of(args[0]).toAbsolutePath().normalize())
                || output.equals(Path.of(args[1]).toAbsolutePath().normalize())) {
            throw new IllegalArgumentException("输出 SWF 使用独立的新文件路径");
        }
        SWF reference = load(Path.of(args[0]));
        SWF input = load(Path.of(args[1]));
        Map<String, Map<String, String>> before = new LinkedHashMap<>();
        for (String name : METHODS.keySet()) {
            ClassView owner = find(input, name);
            before.put(name, owner.digests());
            Map<String, String> expected = find(reference, name).digests();
            for (String method : METHODS.get(name)) {
                if (!expected.get(method).equals(before.get(name).get(method))) {
                    throw new IllegalStateException("待补丁资源方法与参考输入不同: " + name + "." + method);
                }
            }
        }
        ClassView reader = find(input, READER);
        requireFileSize(reader, "fileExistsSync", 4, false);
        recoverFile(reader);
        ClassView loader = find(input, LOADER);
        requireFileSize(loader, "startDownloadFile", 4, false);
        targetFile(loader);
        reopenCachedFile(loader);
        clearRecoveredFileCache(loader, "successHandler");
        clearRecoveredFileCache(loader, "contentFileReadComplete");
        requireFileSize(find(input, CHECKER), "update", 4, true);
        ClassView recoverer = find(input, RECOVERER);
        orderRecoveryCompletion(recoverer);
        clearRecoveryRecordsOnComplete(recoverer);
        verify(input, before);
        Files.createDirectories(output.getParent());
        try (OutputStream stream = Files.newOutputStream(output)) { input.saveTo(stream); }
        verify(load(output), before);
        Files.writeString(Path.of(args[3]), "{\"recoversEmptyDownloadedFiles\":true,\"checksManifestSize\":true,\"retainsRecoveryInfoUntilComplete\":true,\"platformManifest\":\"10939-android_medium.csv\"}\n", StandardCharsets.UTF_8);
        System.out.println("空缓存文件通过正式清单补取, 素材检查比较文件长度, 恢复完成后清除重试记录");
    }
    // //// /保存并检查通用缓存补救方法 ////

    // //// 让存在性条件同时约束文件长度 [@x380kkm 2026-09-27] ////
    private static void requireFileSize(ClassView owner, String name, int fileRegister, boolean manifestSize) {
        MethodBody body = owner.method(name);
        List<Integer> matches = new ArrayList<>();
        for (int index = 0; index < body.getCode().code.size(); index++) {
            AVM2Instruction instruction = body.getCode().code.get(index);
            if (isProperty(owner.abc(), instruction, "exists")) { matches.add(index); }
        }
        if (manifestSize && matches.size() != 2) { throw new IllegalStateException("素材检查存在性条件数量异常"); }
        int selected = manifestSize ? matches.get(0) : only(matches, name);
        Builder code = new Builder(owner.abc());
        code.op("dup");
        AVM2Instruction absent = code.op("iffalse", 0);
        code.op("pop");
        code.local(fileRegister); code.property("size");
        if (manifestSize) {
            code.local(3); code.property("2"); code.op("convert_d"); code.op("equals");
        } else {
            code.op("pushbyte", 0); code.op("greaterthan");
        }
        code.toHere(absent);
        insert(body, selected + 1, code.instructions);
        ((Tag) owner.tag()).setModified(true);
    }
    // //// /让存在性条件同时约束文件长度 ////

    // //// 从下载路径生成受清单约束的补取请求 [@x380kkm 2026-09-27] ////
    private static void recoverFile(ClassView owner) {
        Builder code = new Builder(owner.abc());
        code.scope();
        code.local(0); code.local(1); code.local(2); code.call("resolveFiles", 2); code.store(6);
        code.op("newarray", 0); code.store(7);
        int loop = code.position();
        // AVM2 的回跳目标携带标签.
        code.op("label");
        code.local(6); code.property("length"); AVM2Instruction end = code.op("iffalse", 0);
        code.local(6); code.call("shift", 0); code.op("convert_s"); code.store(8);
        code.local(8); code.text("/production/"); code.call("indexOf", 1); code.store(9);
        code.local(9); code.op("pushbyte", 0); AVM2Instruction noProduction = code.op("iflt", 0);
        code.local(7); code.local(0); code.property("assetDownloader"); code.call("getBaseUrl", 0);
        code.text(MANIFEST_PATH); code.op("add");
        code.lex("StringTools");
        code.local(8); code.local(9); code.op("pushbyte", 1); code.op("add"); code.call("substr", 1);
        code.text("bundle/"); code.text("upload/"); code.call("replace", 3); code.op("add");
        code.callVoid("push", 1);
        code.toHere(noProduction);
        code.toPosition(code.op("jump", 0), loop); code.toHere(end);

        code.local(1); code.text("dynamic/"); code.op("pushbyte", 0); code.call("lastIndexOf", 2);
        code.op("pushbyte", 0); AVM2Instruction normal = code.op("ifne", 0);
        code.local(1); code.text("dynamic/error/not_found"); AVM2Instruction notFound = code.op("ifeq", 0);
        code.local(2); code.text(".png"); AVM2Instruction notPng = code.op("ifne", 0);
        code.lex("pinball.common.tools._FunctionTools.BindImpl5_0");
        code.local(0); code.property("readAssetFileIfExists"); code.text("dynamic/error/not_found");
        code.local(2); code.local(3); code.local(4); code.local(5); code.op("construct", 6);
        code.property("execute"); code.store(10); AVM2Instruction callbackReady = code.op("jump", 0);
        code.toHere(normal); code.toHere(notFound); code.toHere(notPng);
        code.lex("pinball.common.tools._FunctionTools.BindImpl3_0");
        code.local(5); code.local(1); code.local(2); code.local(4); code.op("construct", 4);
        code.property("execute"); code.store(10); code.toHere(callbackReady);

        code.lex(LOADER); code.local(0); code.property("devConfig"); code.local(0); code.local(7);
        code.lex(LOADER); code.local(3); code.call("ignorePathForConvert", 1);
        code.local(4); code.local(10); code.op("construct", 6); code.op("pop"); code.op("returnvoid");
        code.write(owner.method("failedReadAssetFileHandler"), 11);
        ((Tag) owner.tag()).setModified(true);
    }
    // //// /从下载路径生成受清单约束的补取请求 ////

    // //// 将补取内容写回客户端下载缓存 [@x380kkm 2026-09-27] ////
    private static void targetFile(ClassView owner) {
        MethodBody body = owner.method("getDownloadedContentFile");
        Builder code = new Builder(owner.abc());
        code.scope();
        code.local(1); code.text(URL_MARKER); code.call("indexOf", 1); code.store(2);
        code.local(2); code.op("pushbyte", 0); AVM2Instruction original = code.op("iflt", 0);
        code.lex("flash.filesystem.File");
        code.local(0); code.property("fileReader"); code.property("assetDownloader"); code.call("getDownloadedAssetDir", 0);
        code.text("/"); code.op("add"); code.local(1); code.local(2); code.op("pushbyte", URL_MARKER.length()); code.op("add");
        code.call("substr", 1); code.op("add"); code.op("construct", 1); code.store(3);
        code.local(3); code.property("parent"); code.callVoid("createDirectory", 0);
        code.local(3); code.op("returnvalue"); code.toHere(original);
        code.local(1); code.local(1); code.text("asset"); code.call("indexOf", 1); code.call("substring", 1); code.store(1);
        code.local(0); code.property("devConfig"); code.call("getDownloadedAssetDirectory", 0);
        code.text("intermediate_download/"); code.local(1); code.op("add"); code.call("resolvePath", 1); code.op("returnvalue");
        code.write(body, 4); ((Tag) owner.tag()).setModified(true);
    }
    // //// /将补取内容写回客户端下载缓存 ////

    // //// 重读缓存前检查实际文件并补取损坏内容 [@x380kkm 2026-09-27] ////
    private static void reopenCachedFile(ClassView owner) {
        Builder code = new Builder(owner.abc());
        code.scope();
        code.local(0); code.local(1); code.call("getDownloadedContentFile", 1); code.store(2);
        code.local(2); code.property("exists"); AVM2Instruction absent = code.op("iffalse", 0);
        code.local(2); code.property("isDirectory"); AVM2Instruction directory = code.op("iftrue", 0);
        code.local(2); code.property("size"); code.op("pushbyte", 0); AVM2Instruction empty = code.op("ifle", 0);
        code.lex("pinball.asset.file.FileStreamWithCounter"); code.op("construct", 0); code.store(3);
        code.local(3); code.text("complete");
        code.lex("pinball.common.tools._FunctionTools.BindImpl3_1");
        code.local(0); code.property("contentFileReadComplete"); code.local(3); code.local(1);
        code.op("construct", 3); code.property("execute"); code.callVoid("addEventListener", 2);
        code.local(3); code.local(2); code.text("read"); code.callVoid("openAsync", 2); code.op("returnvoid");
        code.toHere(absent); code.toHere(directory); code.toHere(empty);
        code.local(0); code.callVoid("_startDownloadFile", 0); code.op("returnvoid");
        code.write(owner.method("openCachedFile"), 4); ((Tag) owner.tag()).setModified(true);
    }
    // //// /重读缓存前检查实际文件并补取损坏内容 ////

    // //// 在补取回调中刷新文件存在性缓存 [@x380kkm 2026-09-27] ////
    private static void clearRecoveredFileCache(ClassView owner, String method) {
        Builder code = new Builder(owner.abc());
        code.local(0); code.property("fileReader"); code.callVoid("clearExistsCashe", 0);
        insert(owner.method(method), 0, code.instructions);
        ((Tag) owner.tag()).setModified(true);
    }

    // //// 恢复实例按注册顺序提交完成回调 [@x380kkm 2026-09-27] ////
    private static void orderRecoveryCompletion(ClassView owner) {
        MethodBody run = owner.method("run");
        List<AVM2Instruction> original = run.getCode().code;
        int registration = callIndex(owner, "run", "addCompleteHandler") - 6;
        if (registration < 1 || !original.get(registration - 1).definition.instructionName.equals("initproperty")
                || !isProperty(owner.abc(), original.get(registration + 1), "progressManager")
                || !isProperty(owner.abc(), original.get(registration + 2), "waitCompleteImpl")
                || !isProperty(owner.abc(), original.get(registration + 4), "completeHandler")
                || callIndex(owner, "run", "attachChild") != registration + 10) {
            throw new IllegalStateException("恢复进度回调注册边界不匹配");
        }
        Builder code = new Builder(owner.abc());
        code.local(0); code.property("progressManager"); code.property("waitCompleteImpl");
        code.property("completedDispatcher");
        code.lex("jp.sipo.gipo.core.handler.AddBehaviorPreset"); code.property("addTail");
        code.op("setproperty", owner.abc().constants.getPublicQnameId("addBehavior", true));
        insert(run, registration, code.instructions);
        ((Tag) owner.tag()).setModified(true);
    }
    // //// /恢复实例按注册顺序提交完成回调 ////

    // //// 将恢复记录提交限定到存活任务的完成回调 [@x380kkm 2026-09-27] ////
    private static void clearRecoveryRecordsOnComplete(ClassView owner) {
        MethodBody dispose = owner.method("dispose");
        MethodBody complete = owner.method("completeHandler");
        if (dispose.init_scope_depth != complete.init_scope_depth
                || dispose.max_scope_depth != complete.max_scope_depth) {
            throw new IllegalStateException("恢复方法的作用域不一致");
        }
        List<AVM2Instruction> disposal = new ArrayList<>(dispose.getCode().code);
        List<AVM2Instruction> completion = new ArrayList<>(complete.getCode().code);
        int cleanupStart = callIndex(owner, "dispose", "checkPhaseBeforeDispose") - 2;
        int metadataStart = callIndex(owner, "dispose", "removeFrameHandler") + 1;
        if (cleanupStart < 2 || !isProperty(owner.abc(), disposal.get(cleanupStart + 1), "gear")
                || callIndex(owner, "dispose", "readInfoFile") != metadataStart + 2
                || !disposal.get(disposal.size() - 1).definition.instructionName.equals("returnvoid")
                || completion.size() != 5 || callIndex(owner, "completeHandler", "dispose") != 3) {
            throw new IllegalStateException("恢复记录写入边界与生命周期方法不匹配");
        }
        Builder cleanup = new Builder(owner.abc());
        cleanup.scope();
        copyInstructions(cleanup, disposal, cleanupStart, metadataStart);
        cleanup.op("returnvoid");
        Builder success = new Builder(owner.abc());
        success.scope();
        success.local(0); success.property("gear"); success.call("checkPhaseBeforeDispose", 0);
        AVM2Instruction disposed = success.op("iffalse", 0);
        copyInstructions(success, disposal, 2, cleanupStart);
        copyInstructions(success, completion, 2, completion.size() - 1);
        copyInstructions(success, disposal, metadataStart, disposal.size() - 1);
        success.toHere(disposed);
        success.op("returnvoid");
        int registers = dispose.max_regs;
        cleanup.write(dispose, 1);
        success.write(complete, registers);
        ((Tag) owner.tag()).setModified(true);
    }

    private static void copyInstructions(Builder target, List<AVM2Instruction> source, int start, int end) {
        for (int index = start; index < end; index++) { target.instructions.add(source.get(index).clone()); }
    }

    private static int callIndex(ClassView owner, String method, String name) {
        List<Integer> matches = new ArrayList<>();
        List<AVM2Instruction> code = owner.method(method).getCode().code;
        for (int index = 0; index < code.size(); index++) {
            if (isCall(owner.abc(), code.get(index), name)) { matches.add(index); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("恢复方法调用数量异常: " + method + "." + name); }
        return matches.get(0);
    }

    private static boolean isCall(ABC abc, AVM2Instruction instruction, String name) {
        return (instruction.definition.instructionName.equals("callproperty")
                || instruction.definition.instructionName.equals("callpropvoid"))
                && name.equals(abc.constants.getString(abc.constants.getMultiname(instruction.operands[0]).name_index));
    }
    // //// /将恢复记录提交限定到存活任务的完成回调 ////

    // //// 保留已有跳转和异常表的相对位置 [@x380kkm 2026-09-27] ////
    private static void insert(MethodBody body, int position, List<AVM2Instruction> instructions) {
        Map<AVM2Instruction, Integer> offsets = new LinkedHashMap<>();
        for (AVM2Instruction instruction : instructions) {
            if (instruction.isBranch() || instruction.isJump()) { offsets.put(instruction, instruction.operands[0]); }
        }
        for (int index = 0; index < instructions.size(); index++) {
            body.getCode().insertInstruction(position + index, instructions.get(index), true, body);
        }
        offsets.forEach((instruction, offset) -> instruction.operands[0] = offset);
        body.max_stack += 4;
        body.setModified();
        body.getCode().checkValidOffsets(body);
    }

    private static int only(List<Integer> values, String name) {
        if (values.size() != 1) { throw new IllegalStateException("文件存在性条件数量异常: " + name + " " + values.size()); }
        return values.get(0);
    }

    private static boolean isProperty(ABC abc, AVM2Instruction instruction, String name) {
        return instruction.definition.instructionName.equals("getproperty") && instruction.operands.length == 1
                && name.equals(abc.constants.getString(abc.constants.getMultiname(instruction.operands[0]).name_index));
    }

    private static void verify(SWF swf, Map<String, Map<String, String>> before) throws Exception {
        for (String name : METHODS.keySet()) {
            ClassView owner = find(swf, name);
            if (!AbcMethodDigest.changedMethods(before.get(name), owner.digests()).equals(METHODS.get(name))) {
                throw new IllegalStateException("资源补救方法范围异常: " + name);
            }
            for (String method : METHODS.get(name)) { owner.method(method).getCode().checkValidOffsets(owner.method(method)); }
        }
        requireSizeRead(find(swf, READER), "fileExistsSync");
        requireSizeRead(find(swf, LOADER), "startDownloadFile");
        requireSizeRead(find(swf, LOADER), "openCachedFile");
        requireSizeRead(find(swf, CHECKER), "update");
        verifyRecoveryRecordLifecycle(find(swf, RECOVERER));
    }

    // //// 检查保存后的恢复记录提交和资源释放分支 [@x380kkm 2026-09-27] ////
    private static void verifyRecoveryRecordLifecycle(ClassView owner) {
        List<AVM2Instruction> run = owner.method("run").getCode().code;
        List<Integer> callbackOrder = new ArrayList<>();
        for (int index = 0; index < run.size(); index++) {
            if (isProperty(owner.abc(), run.get(index), "addTail")) { callbackOrder.add(index); }
        }
        int ordered = only(callbackOrder, "AssetRecovering.run.addTail");
        if (!isProperty(owner.abc(), run.get(ordered - 2), "completedDispatcher")
                || !run.get(ordered + 1).definition.instructionName.equals("setproperty")
                || ordered + 1 >= callIndex(owner, "run", "addCompleteHandler")
                || callIndex(owner, "run", "addCompleteHandler") >= callIndex(owner, "run", "attachChild")) {
            throw new IllegalStateException("恢复完成回调顺序异常");
        }
        callIndex(owner, "dispose", "checkPhaseBeforeDispose");
        callIndex(owner, "dispose", "remove");
        callIndex(owner, "dispose", "removeFrameHandler");
        if (owner.method("dispose").getCode().code.stream().anyMatch(instruction ->
                isCall(owner.abc(), instruction, "readInfoFile") || isCall(owner.abc(), instruction, "writeInfoFile"))) {
            throw new IllegalStateException("资源释放路径仍写入恢复记录");
        }
        int phaseCheck = callIndex(owner, "completeHandler", "checkPhaseBeforeDispose");
        int dispose = callIndex(owner, "completeHandler", "dispose");
        int read = callIndex(owner, "completeHandler", "readInfoFile");
        int write = callIndex(owner, "completeHandler", "writeInfoFile");
        List<AVM2Instruction> code = owner.method("completeHandler").getCode().code;
        AVM2Instruction guard = code.get(phaseCheck + 1);
        if (!(phaseCheck < dispose && dispose < read && read < write)
                || !guard.definition.instructionName.equals("iffalse")
                || guard.getTargetAddress() != code.get(code.size() - 1).getAddress()) {
            throw new IllegalStateException("恢复完成路径的存活检查或记录提交顺序异常");
        }
    }
    // //// /检查保存后的恢复记录提交和资源释放分支 ////

    private static void requireSizeRead(ClassView owner, String method) {
        if (owner.method(method).getCode().code.stream().noneMatch(instruction -> isProperty(owner.abc(), instruction, "size"))) {
            throw new IllegalStateException("资源读取方法缺少文件长度判断: " + method);
        }
    }

    private static SWF load(Path path) throws Exception {
        try (InputStream stream = Files.newInputStream(path)) { return new SWF(stream, false); }
    }

    private static ClassView find(SWF swf, String name) {
        List<ClassView> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            int index = tag.getABC().findClassByName(name);
            if (index >= 0) { matches.add(new ClassView(tag, tag.getABC(), index)); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("资源类数量异常: " + name); }
        return matches.get(0);
    }

    private record ClassView(ABCContainerTag tag, ABC abc, int index) {
        MethodBody method(String name) { return AbcMethodDigest.findInstanceMethod(abc, index, name); }
        Map<String, String> digests() throws Exception { return AbcMethodDigest.digestInstanceMethods(abc, index); }
    }
    // //// /保留已有跳转和异常表的相对位置 ////

    // //// 组装资源读取方法的局部指令 [@x380kkm 2026-09-27] ////
    private static final class Builder {
        final ABC abc;
        final List<AVM2Instruction> instructions = new ArrayList<>();
        Builder(ABC abc) { this.abc = abc; }
        AVM2Instruction op(String name, int... operands) {
            for (InstructionDefinition definition : AVM2Code.instructionSet) {
                if (definition != null && definition.instructionName.equals(name)) {
                    AVM2Instruction result = new AVM2Instruction(0, definition, operands);
                    instructions.add(result); return result;
                }
            }
            throw new IllegalArgumentException("未知 AVM2 指令: " + name);
        }
        void local(int number) { op("getlocal", number); }
        void store(int number) { op("setlocal", number); }
        void property(String name) { op("getproperty", abc.constants.getPublicQnameId(name, true)); }
        void call(String name, int count) { op("callproperty", abc.constants.getPublicQnameId(name, true), count); }
        void callVoid(String name, int count) { op("callpropvoid", abc.constants.getPublicQnameId(name, true), count); }
        void text(String value) { op("pushstring", abc.constants.getStringId(value, true)); }
        void lex(String name) {
            int end = name.lastIndexOf('.');
            op("getlex", abc.constants.getQnameId(name.substring(end + 1), Namespace.KIND_PACKAGE, end < 0 ? "" : name.substring(0, end), true));
        }
        void scope() { local(0); op("pushscope"); }
        int position() { return instructions.stream().mapToInt(AVM2Instruction::getBytesLength).sum(); }
        void toHere(AVM2Instruction branch) { toPosition(branch, position()); }
        void toPosition(AVM2Instruction branch, int target) {
            int end = 0;
            for (AVM2Instruction instruction : instructions) { end += instruction.getBytesLength(); if (instruction == branch) { break; } }
            branch.operands[0] = target - end;
        }
        void write(MethodBody body, int registers) {
            if (body.exceptions.length != 0) { throw new IllegalStateException("资源方法包含额外异常处理"); }
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            Set<Integer> labels = new java.util.HashSet<>();
            for (AVM2Instruction instruction : instructions) {
                if (instruction.definition.instructionName.equals("label")) { labels.add(bytes.size()); }
                if ((instruction.isBranch() || instruction.isJump()) && instruction.operands[0] < 0
                        && !labels.contains(bytes.size() + instruction.getBytesLength() + instruction.operands[0])) {
                    throw new IllegalStateException("资源方法的回跳目标缺少标签");
                }
                bytes.writeBytes(instruction.getBytes());
            }
            body.setCodeBytes(bytes.toByteArray());
            body.max_regs = registers; body.max_stack = 16;
            body.getCode().checkValidOffsets(body);
        }
    }
    // //// /组装资源读取方法的局部指令 ////
}
