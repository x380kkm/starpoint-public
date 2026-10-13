// audience: internal
// # local-payment-abc-patch
// 此工具将 CN Android 商品购买接入本地订单, 并保留原有付款完成响应解析和界面回调.
// Java 17 和 FFDec 24 读取方法体. SDK 调用区间的等长替换保留分支目标, 商品条件返回类型对应的常量.
// 商店列表刷新按当前横幅行同步原按钮组, 已有按钮保留回调和可用状态.

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.avm2.parser.pcode.ASM3Parser;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.abc.types.Multiname;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import com.jpexs.decompiler.flash.tags.Tag;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.StringReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class LocalPaymentAbcPatch {
    static final String PAYMENT = "pinball.payment.real.RealPayment";
    static final String PLATFORM = "pinball.payment.gbits.GbitsAndroidRealPayment";
    static final String FINISH = "pinball.remote.payment.finish.PaymentFinishRealRemote";
    static final String PRODUCT = "pinball.common.data.payment.PaymentProductLogic";
    static final String SHOP_VIEW = "pinball.dialog.stoneBuy.StoneBuyDialogView";
    static final Map<String, Set<String>> METHODS = Map.of(
            PAYMENT, Set.of("prepare", "getPurchasesHandler"),
            PLATFORM, Set.of("run", "initialize", "dispose", "purchase"),
            FINISH, Set.of("<iinit>"),
            PRODUCT, Set.of("canBeBought", "isDisplayable"),
            SHOP_VIEW, Set.of("refreshList"));

    private LocalPaymentAbcPatch() { }

    // //// 保存本地购买指令并核对付款完成接口 [@x380kkm 2026-09-22] ////
    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException("usage: LocalPaymentAbcPatch <reference.swf> <input.swf> <output.swf> <evidence.json>");
        }
        Path outputPath = Path.of(args[2]).toAbsolutePath().normalize();
        if (Files.exists(outputPath) || outputPath.equals(Path.of(args[0]).toAbsolutePath().normalize())
                || outputPath.equals(Path.of(args[1]).toAbsolutePath().normalize())) {
            throw new IllegalArgumentException("输出 SWF 使用独立的新文件路径");
        }
        SWF reference = load(Path.of(args[0]));
        SWF input = load(Path.of(args[1]));
        Map<String, Map<String, String>> before = new LinkedHashMap<>();
        for (String name : METHODS.keySet()) {
            ClassView target = find(input, name);
            before.put(name, target.digests());
            ClassView original = find(reference, name);
            AbcMethodDigest.requireSameInstanceMethods(original.abc(), original.index(),
                    target.abc(), target.index(), "付款方法与参考输入不同");
        }
        patch(input);
        verify(input, before);
        Files.createDirectories(outputPath.getParent());
        Path temporary = Files.createTempFile(outputPath.getParent(), "local-payment-", ".swf");
        try (OutputStream stream = Files.newOutputStream(temporary)) { input.saveTo(stream); }
        verify(load(temporary), before);
        Files.move(temporary, outputPath);
        Path evidencePath = Path.of(args[3]);
        Files.createDirectories(evidencePath.toAbsolutePath().getParent());
        Files.writeString(evidencePath, "{\"localOrderReceipt\":\"productId:orderId\","
                + "\"purchaseCompletion\":\"OnQueryPayStatusSuccess\",\"nativePaymentCalls\":0,"
                + "\"finishResponseParserPreserved\":true,\"localProductEligibility\":\"Possible\",\"productPeriodPreserved\":true,"
                + "\"bannerButtonsFollowCurrentList\":true,\"changedMethods\":10}\n", StandardCharsets.UTF_8);
        System.out.println("商品购买调用本地付款完成接口, 原响应解析和消费回调保持不变");
    }
    // //// /保存本地购买指令并核对付款完成接口 ////

    // //// 连接订单完成并移除付款流程中的原生 SDK 调用 [@x380kkm 2026-09-22] ////
    static void patch(SWF swf) {
        ClassView payment = find(swf, PAYMENT);
        removeNativeGuard(payment, "prepare");
        removeNativeGuard(payment, "getPurchasesHandler");
        ClassView platform = find(swf, PLATFORM);
        platform.method("run").setCodeBytes(new byte[] {0x47});
        platform.method("dispose").setCodeBytes(new byte[] {0x47});
        MethodBody initialize = platform.method("initialize");
        int nativeIndex = nativeStart(platform, initialize);
        requireOpcodes(initialize, nativeIndex, "getlex", "callpropvoid");
        replace(initialize, nativeIndex, nativeIndex + 2, new byte[0]);

        MethodBody purchase = platform.method("purchase");
        nativeIndex = nativeStart(platform, purchase);
        requireOpcodes(purchase, nativeIndex, "getlex", "callproperty", "coerce", "coerce", "setlocal3");
        replace(purchase, nativeIndex, nativeIndex + 5, new byte[0]);
        int payIndex = namedInstruction(platform, purchase, "callpropvoid", "pay");
        requireOpcodes(purchase, payIndex - 2, "getlocal3", "getlocal", "callpropvoid");
        int successIndex = namedInstruction(platform, platform.method("OnQuestPayResult"), "callpropvoid", "OnQueryPayStatusSuccess");
        int success = platform.method("OnQuestPayResult").getCode().code.get(successIndex).operands[0];
        ByteArrayOutputStream call = new ByteArrayOutputStream();
        call.write(0xd0);
        call.write(0x4f);
        writeU30(call, success);
        call.write(0);
        replace(purchase, payIndex - 2, payIndex + 1, call.toByteArray());

        ClassView finish = find(swf, FINISH);
        MethodBody constructor = finish.method("<iinit>");
        int begin = nativeStart(finish, constructor);
        int end = stringInstruction(finish, constructor, "channelNo");
        if (end <= begin || namedInstruction(finish, constructor, "callproperty", "getDeviceInfo") >= end) {
            throw new IllegalStateException("付款设备信息区间不符合客户端定义");
        }
        replace(constructor, begin, end, new byte[0]);
        allowLocalProducts(find(swf, PRODUCT));
        synchronizeStoneBuyList(find(swf, SHOP_VIEW));
        for (String name : METHODS.keySet()) { ((Tag) find(swf, name).tag()).setModified(true); }
    }

    // //// 返回本地商品的可展示和可领取状态 [@x380kkm 2026-09-22] ////
    private static void allowLocalProducts(ClassView owner) {
        MethodBody purchase = owner.method("canBeBought");
        int state = multinameReference(owner, purchase, "getlex", "StoneBuyableState");
        int possible = multinameReference(owner, purchase, "getproperty", "Possible");
        ByteArrayOutputStream code = new ByteArrayOutputStream();
        code.write(0xd0);
        code.write(0x30);
        code.write(0x60);
        writeU30(code, state);
        code.write(0x66);
        writeU30(code, possible);
        code.write(0x48);
        replaceBody(purchase, code.toByteArray());
        replaceBody(owner.method("isDisplayable"), new byte[] {(byte) 0xd0, 0x30, 0x26, 0x48});
    }

    private static void replaceBody(MethodBody body, byte[] code) {
        if (body.exceptions.length != 0) { throw new IllegalStateException("商品条件包含异常表"); }
        body.setCodeBytes(code);
        body.getCode().checkValidOffsets(body);
    }

    private static int multinameReference(ClassView owner, MethodBody body, String opcode, String name) {
        Set<Integer> matches = new java.util.HashSet<>();
        for (AVM2Instruction instruction : body.getCode().code) {
            if (!instruction.definition.instructionName.equals(opcode)) { continue; }
            Multiname value = owner.abc().constants.getMultiname(instruction.operands[0]);
            if (value != null && value.hasOwnName() && name.equals(owner.abc().constants.getString(value.name_index))) {
                matches.add(instruction.operands[0]);
            }
        }
        if (matches.size() != 1) { throw new IllegalStateException("商品条件符号解析不唯一: " + name); }
        return matches.iterator().next();
    }
    // //// /返回本地商品的可展示和可领取状态 ////

    // //// 在商店重绘前同步横幅按钮并保留已有按钮 [@x380kkm 2026-09-28] ////
    static void synchronizeStoneBuyList(ClassView owner) {
        MethodBody body = owner.method("refreshList");
        requireOpcodes(body, 0, "getlocal0", "pushscope", "findproperty", "getproperty", "callpropvoid", "returnvoid");
        namedInstruction(owner, body, "callpropvoid", "refreshAllCells");
        if (body.exceptions.length != 0) { throw new IllegalStateException("商店刷新方法包含异常表"); }
        String source = """
                getlocal0
                pushscope
                getlocal0
                getproperty QName(PackageNamespace(""), "peek")
                callproperty QName(PackageNamespace(""), "getList"), 0
                coerce QName(PackageNamespace("pinball.ui.component.list.shop"), "StoneBuyListLogic")
                setlocal1
                getlocal1
                callproperty QName(PackageNamespace(""), "getExtraButtonGroup"), 0
                coerce QName(PackageNamespace("pinball.ui.component.button"), "ButtonGroupLogic")
                setlocal2
                getlocal1
                callproperty QName(PackageNamespace(""), "get_dataSource"), 0
                coerce QName(PackageNamespace(""), "Array")
                setlocal3
                getlocal2
                callproperty QName(PackageNamespace(""), "getKinds"), 0
                coerce QName(PackageNamespace(""), "Array")
                setlocal 4
                getlocal3
                getproperty QName(PackageNamespace(""), "length")
                convert_i
                setlocal 5
                getlocal 4
                getproperty QName(PackageNamespace(""), "length")
                convert_i
                setlocal 6
            removeLoop:
                getlocal 6
                pushbyte 0
                ifle appendStart
                declocal_i 6
                getlocal 4
                getlocal 6
                getproperty MultinameL([PackageNamespace("")])
                convert_i
                setlocal 7
                getlocal 7
                pushbyte 0
                iflt removeButton
                getlocal 7
                getlocal 5
                ifge removeButton
                getlocal3
                getlocal 7
                getproperty MultinameL([PackageNamespace("")])
                coerce QName(PackageNamespace("pinball.common.data.payment"), "PaymentProductLogic")
                callproperty QName(PackageNamespace(""), "get_displayKind"), 0
                getproperty QName(PackageNamespace(""), "index")
                pushbyte 1
                ifeq removeLoop
            removeButton:
                getlocal2
                getlocal 7
                callpropvoid QName(PackageNamespace(""), "remove"), 1
                jump removeLoop
            appendStart:
                pushbyte 0
                setlocal 6
            appendLoop:
                getlocal 6
                getlocal 5
                ifge render
                getlocal3
                getlocal 6
                getproperty MultinameL([PackageNamespace("")])
                coerce QName(PackageNamespace("pinball.common.data.payment"), "PaymentProductLogic")
                callproperty QName(PackageNamespace(""), "get_displayKind"), 0
                getproperty QName(PackageNamespace(""), "index")
                pushbyte 1
                ifne appendNext
                getlocal2
                getlocal 6
                callproperty QName(PackageNamespace(""), "exists"), 1
                iftrue appendNext
                getlocal2
                getlocal 6
                callpropvoid QName(PackageNamespace(""), "append"), 1
            appendNext:
                inclocal_i 6
                jump appendLoop
            render:
                getlocal0
                getproperty QName(PackageNamespace(""), "dialogListView")
                callpropvoid QName(PackageNamespace(""), "refreshAllCells"), 0
                returnvoid
                """;
        try {
            body.setCode(ASM3Parser.parse(owner.abc(), new StringReader(source), null, body,
                    owner.abc().method_info.get(body.method_info)));
        } catch (Exception error) {
            throw new IllegalStateException("商店横幅按钮指令解析失败", error);
        }
        body.max_regs = 8;
        body.max_stack = Math.max(body.max_stack, 3);
        body.getCode().checkValidOffsets(body);
    }
    // //// /在商店重绘前同步横幅按钮并保留已有按钮 ////

    private static void removeNativeGuard(ClassView owner, String name) {
        MethodBody body = owner.method(name);
        int start = nativeStart(owner, body);
        requireOpcodes(body, start, "getlex", "callproperty", "coerce", "coerce", "setlocal2",
                "pushnull", "coerce", "getlocal2", "ifeq");
        replace(body, start, start + 9, new byte[0]);
    }
    // //// /连接订单完成并移除付款流程中的原生 SDK 调用 ////

    // //// 等长替换完整指令区间 [@x380kkm 2026-09-22] ////
    private static void replace(MethodBody body, int from, int to, byte[] replacement) {
        List<AVM2Instruction> code = body.getCode().code;
        byte[] bytes = body.getCodeBytes().clone();
        int begin = Math.toIntExact(code.get(from).getAddress());
        int end = to == code.size() ? bytes.length : Math.toIntExact(code.get(to).getAddress());
        if (body.exceptions.length != 0 || replacement.length > end - begin || begin < 0 || begin >= end) {
            throw new IllegalStateException("付款方法的替换区间或异常表不符合预期");
        }
        Arrays.fill(bytes, begin, end, (byte) 0x02);
        System.arraycopy(replacement, 0, bytes, begin, replacement.length);
        body.setCodeBytes(bytes);
        body.getCode().checkValidOffsets(body);
    }

    private static void requireOpcodes(MethodBody body, int start, String... expected) {
        List<AVM2Instruction> code = body.getCode().code;
        if (start < 0 || start + expected.length > code.size()) { throw new IllegalStateException("付款指令区间越界"); }
        for (int index = 0; index < expected.length; index++) {
            if (!code.get(start + index).definition.instructionName.equals(expected[index])) {
                throw new IllegalStateException("付款指令不符合预期: " + expected[index]);
            }
        }
    }

    private static void writeU30(ByteArrayOutputStream stream, int value) {
        do {
            int part = value & 0x7f;
            value >>>= 7;
            stream.write(value == 0 ? part : part | 0x80);
        } while (value != 0);
    }
    // //// /等长替换完整指令区间 ////

    // //// 唯一定位付款指令引用 [@x380kkm 2026-09-22] ////
    private static int nativeStart(ClassView owner, MethodBody body) {
        return namedInstruction(owner, body, "getlex", "LeitingSDKExtension");
    }

    private static int namedInstruction(ClassView owner, MethodBody body, String opcode, String name) {
        List<Integer> matches = new ArrayList<>();
        List<AVM2Instruction> code = body.getCode().code;
        for (int index = 0; index < code.size(); index++) {
            AVM2Instruction instruction = code.get(index);
            if (!instruction.definition.instructionName.equals(opcode)) { continue; }
            Multiname multiname = owner.abc().constants.getMultiname(instruction.operands[0]);
            if (multiname != null && multiname.hasOwnName()
                    && name.equals(owner.abc().constants.getString(multiname.name_index))) { matches.add(index); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("付款指令引用数量异常: " + name + ": " + matches.size()); }
        return matches.get(0);
    }

    private static int stringInstruction(ClassView owner, MethodBody body, String text) {
        List<Integer> matches = new ArrayList<>();
        List<AVM2Instruction> code = body.getCode().code;
        for (int index = 0; index < code.size(); index++) {
            AVM2Instruction instruction = code.get(index);
            if (instruction.definition.instructionName.equals("pushstring")
                    && text.equals(owner.abc().constants.getString(instruction.operands[0]))) { matches.add(index); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("付款字符串引用数量异常: " + text); }
        return matches.get(0);
    }
    // //// /唯一定位付款指令引用 ////

    // //// 核对方法边界和本地付款完成调用 [@x380kkm 2026-09-22] ////
    static void verify(SWF swf, Map<String, Map<String, String>> before) throws Exception {
        for (var entry : METHODS.entrySet()) {
            ClassView owner = find(swf, entry.getKey());
            Set<String> changed = AbcMethodDigest.changedMethods(before.get(entry.getKey()), owner.digests());
            if (!changed.equals(entry.getValue())) { throw new IllegalStateException("付款补丁包含额外方法变化: " + changed); }
            for (String name : entry.getValue()) {
                MethodBody body = owner.method(name);
                body.getCode().checkValidOffsets(body);
                for (AVM2Instruction instruction : body.getCode().code) {
                    if (!Set.of("getlex", "callproperty", "callpropvoid").contains(instruction.definition.instructionName)) { continue; }
                    Multiname value = owner.abc().constants.getMultiname(instruction.operands[0]);
                    if (value != null && value.hasOwnName() && Set.of("LeitingSDKExtension", "pay", "getDeviceInfo")
                            .contains(owner.abc().constants.getString(value.name_index))) {
                        throw new IllegalStateException("本地付款方法仍调用原生 SDK: " + name);
                    }
                }
            }
        }
        ClassView platform = find(swf, PLATFORM);
        namedInstruction(platform, platform.method("purchase"), "callpropvoid", "OnQueryPayStatusSuccess");
        namedInstruction(platform, platform.method("OnQueryPayStatusSuccess"), "callpropvoid", "startPaymentFinish");
        ClassView finish = find(swf, FINISH);
        namedInstruction(finish, finish.method("<iinit>"), "callpropvoid", "startUserRequestDetail");
        ClassView product = find(swf, PRODUCT);
        namedInstruction(product, product.method("canBeBought"), "getproperty", "Possible");
        requireOpcodes(product.method("isDisplayable"), 0, "getlocal0", "pushscope", "pushtrue", "returnvalue");
        namedInstruction(product, product.method("isWithinPeriod"), "callproperty", "isWithin");
        ClassView shopView = find(swf, SHOP_VIEW);
        MethodBody refresh = shopView.method("refreshList");
        namedInstruction(shopView, refresh, "callproperty", "getExtraButtonGroup");
        namedInstruction(shopView, refresh, "callpropvoid", "remove");
        namedInstruction(shopView, refresh, "callpropvoid", "append");
        namedInstruction(shopView, refresh, "callpropvoid", "refreshAllCells");
    }

    static SWF load(Path path) throws Exception {
        try (InputStream stream = Files.newInputStream(path)) { return new SWF(stream, false); }
    }

    static ClassView find(SWF swf, String name) {
        List<ClassView> matches = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            int index = tag.getABC().findClassByName(name);
            if (index >= 0) { matches.add(new ClassView(tag, tag.getABC(), index)); }
        }
        if (matches.size() != 1) { throw new IllegalStateException("付款类数量异常: " + name + ": " + matches.size()); }
        return matches.get(0);
    }

    record ClassView(ABCContainerTag tag, ABC abc, int index) {
        MethodBody method(String name) {
            return name.equals("<iinit>") ? abc.findBody(abc.instance_info.get(index).iinit_index)
                    : AbcMethodDigest.findInstanceMethod(abc, index, name);
        }
        Map<String, String> digests() throws Exception { return AbcMethodDigest.digestInstanceMethods(abc, index); }
    }
    // //// /核对方法边界和本地付款完成调用 ////
}
