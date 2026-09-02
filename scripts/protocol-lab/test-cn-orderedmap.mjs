// audience: internal
// # test-cn-orderedmap
// 此本机测试验证嵌套 orderedmap 解码和规范化 CN 基线扭蛋资产.

import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import zlib from "node:zlib"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"

// //// 验证 CN orderedmap 解码和规范化资产 [@x380kkm 2026-07-23] ////
function verifyGenerator() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-gacha-test-"))
    try {
        const gachaRow = Array(47).fill("")
        Object.assign(gachaRow, {
            0: "normal_test",
            1: "测试扭蛋",
            2: "100",
            3: "dynamic/gacha_list_banner/test",
            4: "0",
            5: "150",
            6: "1500",
            7: "50",
            8: "1200",
            9: "1",
            10: "4",
            11: "normal_rarity",
            13: "0",
            14: "pool_3",
            15: "pool_4",
            16: "pool_5",
            17: "normal",
            18: "normal_guarantee",
            20: "true",
            21: "true",
            27: "20001",
            28: "20002",
            29: "2024-01-01 00:00:00",
            30: "2024-12-31 23:59:59",
            31: "2024-02-01 00:00:00",
            32: "true",
            38: "true",
            39: "tutorial_rarity",
            40: "tutorial_movie",
            41: "100",
            42: "72",
            43: "true",
            44: "true",
            45: "999012",
            46: "true",
        })
        const files = {
            gacha: encodeOrderedMap({ 1: gachaRow }),
            rarity: encodeOrderedMap({ normal_rarity: { 0: [5, 5], 1: [4, 25], 2: [3, 70] } }),
            pool_3: encodeOrderedMap({ pool_3: { 0: [311001, 3, 1, false, false, false, false] } }),
            pool_4: encodeOrderedMap({ pool_4: { 0: [211001, 4, 1, false, false, false, false] } }),
            pool_5: encodeOrderedMap({ pool_5: { 0: [111001, 5, 1, false, false, false, false] } }),
        }
        for (const [name, contents] of Object.entries(files)) fs.writeFileSync(path.join(root, name), contents)
        const output = path.join(root, "cn_gacha.json")
        const generator = fileURLToPath(new URL("./generate-cn-gacha-fixture.mjs", import.meta.url))
        const result = spawnSync(process.execPath, [
            generator,
            "--gacha", path.join(root, "gacha"),
            "--rarity", path.join(root, "rarity"),
            "--map", `pool_3=${path.join(root, "pool_3")}`,
            "--map", `pool_4=${path.join(root, "pool_4")}`,
            "--map", `pool_5=${path.join(root, "pool_5")}`,
            "--output", output,
        ], { encoding: "utf8" })
        assert.equal(result.status, 0, result.stderr)
        const generated = JSON.parse(fs.readFileSync(output, "utf8"))["1"]
        assert.deepEqual({
            stringId: generated.stringId,
            title: generated.title,
            name: generated.name,
            listOrder: generated.listOrder,
            bannerImage: generated.bannerImage,
            pageKind: generated.pageKind,
            tenTimesPerAccountCost: generated.tenTimesPerAccountCost,
            guaranteeNumber: generated.guaranteeNumber,
            onceTicketItemId: generated.onceTicketItemId,
            tenTimesTicketItemId: generated.tenTimesTicketItemId,
            crazyTenTimesTicketItemId: generated.crazyTenTimesTicketItemId,
            wildcardCharacterTicketAvailable: generated.wildcardCharacterTicketAvailable,
            canBeStartDashExchange: generated.canBeStartDashExchange,
            wildcardEquipmentTicketAvailable: generated.wildcardEquipmentTicketAvailable,
            ticketExpiryAtMs: generated.ticketExpiryAtMs,
            showPeriod: generated.showPeriod,
            isComeback: generated.isComeback,
            isStarsGacha: generated.isStarsGacha,
            freemiumGuaranteeAvailable: generated.freemiumGuaranteeAvailable,
            canBeTutorial: generated.canBeTutorial,
            tutorialOddsRarityStringId: generated.tutorialOddsRarityStringId,
            tutorialMovieId: generated.tutorialMovieId,
            tutorialOnceCost: generated.tutorialOnceCost,
            tutorialReasonId: generated.tutorialReasonId,
        }, {
            stringId: "normal_test",
            title: "测试扭蛋",
            name: "测试扭蛋",
            listOrder: 100,
            bannerImage: "dynamic/gacha_list_banner/test",
            pageKind: 0,
            tenTimesPerAccountCost: 1200,
            guaranteeNumber: 1,
            onceTicketItemId: 20001,
            tenTimesTicketItemId: 20002,
            crazyTenTimesTicketItemId: 999012,
            wildcardCharacterTicketAvailable: true,
            canBeStartDashExchange: true,
            wildcardEquipmentTicketAvailable: false,
            ticketExpiryAtMs: Date.parse("2024-02-01T00:00:00+09:00"),
            showPeriod: true,
            isComeback: true,
            isStarsGacha: true,
            freemiumGuaranteeAvailable: true,
            canBeTutorial: true,
            tutorialOddsRarityStringId: "tutorial_rarity",
            tutorialMovieId: "tutorial_movie",
            tutorialOnceCost: 100,
            tutorialReasonId: 72,
        })
        assert.deepEqual(generated.rankRates.normal, [50, 250, 700])
        assert.deepEqual(Object.values(generated.pool).map((pool) => pool[0].rarity), [1000, 1000, 1000])
    } finally {
        fs.rmSync(root, { recursive: true, force: true })
    }
}

const fixture = encodeOrderedMap({
    1: ["normal_1", "开服纪念扭蛋", "value,with,commas", "line1\nline2"],
    normal_rarity: {
        0: [5, 5],
        1: [4, 25],
        2: [3, 70],
    },
})
assert.deepEqual(decodeOrderedMap(fixture), {
    1: ["normal_1", "开服纪念扭蛋", "value,with,commas", "line1\nline2"],
    normal_rarity: {
        0: ["5", "5"],
        1: ["4", "25"],
        2: ["3", "70"],
    },
})

const reservedKeys = decodeOrderedMap(encodeOrderedMap([
    ["__proto__", ["safe"]],
    ["constructor", ["also-safe"]],
]))
assert.equal(Object.getPrototypeOf(reservedKeys), Object.prototype)
assert.deepEqual(reservedKeys.__proto__, ["safe"])
assert.deepEqual(reservedKeys.constructor, ["also-safe"])
assert.throws(() => decodeOrderedMap(encodeOrderedMap([
    ["duplicate", ["first"]],
    ["duplicate", ["second"]],
])), /duplicate key/)

const corrupted = Buffer.from(fixture)
corrupted[corrupted.length - 1] ^= 0xff
assert.throws(() => decodeOrderedMap(corrupted), /cannot be inflated/)
assert.throws(() => decodeOrderedMap(fixture.subarray(0, fixture.length - 1)), /out of bounds|cannot be inflated|data table/)
assert.throws(() => decodeOrderedMap(Buffer.concat([fixture, Buffer.from([0])])), /trailing bytes/)
const rowWithTrailingBytes = Buffer.concat([zlib.deflateSync(Buffer.from("value")), Buffer.from([0])])
assert.throws(() => decodeOrderedMap(encodeOrderedMap([["row", rowWithTrailingBytes]])), /trailing compressed bytes/)
assert.throws(() => decodeOrderedMap(fixture, { maxEntries: 1 }), /entry count/)
assert.throws(() => decodeOrderedMap(fixture, { maxInputBytes: fixture.length - 1 }), /input exceeds/)

let deeplyNested = { leaf: ["value"] }
for (let depth = 0; depth < 8; depth += 1) deeplyNested = { child: deeplyNested }
assert.throws(() => decodeOrderedMap(encodeOrderedMap(deeplyNested), { maxDepth: 4 }), /nesting depth/)
const expandedRow = encodeOrderedMap({ large: ["x".repeat(2048)] })
assert.throws(() => decodeOrderedMap(expandedRow, { maxInflatedBytes: 128 }), /cannot be inflated|budget/)
verifyGenerator()

const cnGacha = JSON.parse(fs.readFileSync(new URL("../../assets/cn_gacha.json", import.meta.url), "utf8"))["1"]
assert.deepEqual(cnGacha.rankRates.normal, [50, 250, 700])
assert.deepEqual(cnGacha.rankRates.multiGuarantee, [167, 833])
assert.deepEqual(Object.keys(cnGacha.pool), ["1", "2", "3"])
assert.deepEqual(Object.values(cnGacha.pool).map((pool) => pool.length), [15, 27, 49])
assert.ok(Object.values(cnGacha.pool).flat().every((item) => item.id > 0 && item.rarity > 0))
assert.ok(Object.values(cnGacha.pool).every((pool) => {
    const total = pool.reduce((sum, item) => sum + item.rarity, 0)
    return total >= 999 && total <= 1001
}))

const runtimeGachas = JSON.parse(fs.readFileSync(new URL("../../assets/gacha.json", import.meta.url), "utf8"))
const poolCounts = (gacha) => ["1", "2", "3"].map((rank) => gacha.pool[rank].length)
const poolSignature = (gacha) => ["1", "2", "3"]
    .map((rank) => gacha.pool[rank].map((item) => item.id).join(","))
    .join("|")
assert.equal(Object.keys(runtimeGachas).length, 584)
assert.deepEqual(runtimeGachas["1"], cnGacha)
const pageKindCounts = Array(9).fill(0)
for (const gacha of Object.values(runtimeGachas)) {
    assert.ok(Number.isInteger(gacha.pageKind) && gacha.pageKind >= 0 && gacha.pageKind <= 8)
    pageKindCounts[gacha.pageKind] += 1
    assert.ok(typeof gacha.stringId === "string" && gacha.stringId.length > 0)
    assert.ok(typeof gacha.title === "string" && gacha.title.length > 0)
    assert.ok(typeof gacha.bannerImage === "string" && gacha.bannerImage.length > 0)
}
assert.deepEqual(pageKindCounts, [535, 20, 1, 4, 13, 1, 0, 0, 10])
assert.ok(Object.values(runtimeGachas).every((gacha) => (
    Number.isSafeInteger(gacha.startAtMs) &&
    Number.isSafeInteger(gacha.endAtMs) &&
    gacha.endAtMs > gacha.startAtMs
)))
assert.deepEqual(poolCounts(runtimeGachas["3"]), [6, 14, 20])
assert.deepEqual(poolCounts(runtimeGachas["155"]), [18, 13, 12])
assert.ok(Object.values(runtimeGachas["155"].pool).flat().every((item) => String(item.id).startsWith(`${6 - item.rank}61`)))
assert.deepEqual(poolCounts(runtimeGachas["157"]), [106, 94, 73])
assert.deepEqual(runtimeGachas["157"].rankRates, {
    normal: [75, 250, 675],
    multiGuarantee: [231, 769],
})
assert.equal(runtimeGachas["157"].startAtMs, Date.parse("2023-01-31T12:00:00+09:00"))
assert.deepEqual(
    Object.values(runtimeGachas["157"].pool).flat()
        .filter((item) => item.isRateUp)
        .map((item) => item.id)
        .sort((left, right) => left - right),
    [111147, 151147, 151153],
)
assert.equal(new Set(["1", "3", "155", "157"].map((id) => poolSignature(runtimeGachas[id]))).size, 4)
assert.deepEqual({
    pageKind: runtimeGachas["2"].pageKind,
    onceTicketItemId: runtimeGachas["2"].onceTicketItemId,
}, { pageKind: 3, onceTicketItemId: 20003 })
assert.deepEqual({
    pageKind: runtimeGachas["100"].pageKind,
    crazyTenTimesTicketItemId: runtimeGachas["100"].crazyTenTimesTicketItemId,
    ticketExpiryAtMs: runtimeGachas["100"].ticketExpiryAtMs,
}, {
    pageKind: 5,
    crazyTenTimesTicketItemId: 999012,
    ticketExpiryAtMs: Date.parse("2050-12-16T11:59:59+09:00"),
})
assert.deepEqual({
    pageKind: runtimeGachas["800000"].pageKind,
    tenTimesPerAccountCost: runtimeGachas["800000"].tenTimesPerAccountCost,
}, { pageKind: 1, tenTimesPerAccountCost: 1500 })
assert.deepEqual({
    canBeTutorial: runtimeGachas["61"].canBeTutorial,
    tutorialOddsRarityStringId: runtimeGachas["61"].tutorialOddsRarityStringId,
    tutorialMovieId: runtimeGachas["61"].tutorialMovieId,
    tutorialOnceCost: runtimeGachas["61"].tutorialOnceCost,
    tutorialReasonId: runtimeGachas["61"].tutorialReasonId,
}, {
    canBeTutorial: true,
    tutorialOddsRarityStringId: "tutorial_rarity",
    tutorialMovieId: "normal_guarantee",
    tutorialOnceCost: 150,
    tutorialReasonId: 72,
})
assert.equal(runtimeGachas["700000"].isComeback, true)
assert.equal(runtimeGachas["80000"].isStarsGacha, true)
assert.equal(runtimeGachas["29"].wildcardCharacterTicketAvailable, true)
assert.equal(runtimeGachas["1512"].canBeStartDashExchange, true)
assert.equal(runtimeGachas["5000"].wildcardEquipmentTicketAvailable, true)
// //// /验证 CN orderedmap 解码和规范化资产 ////

console.log("CN orderedmap and gacha asset tests passed.")
