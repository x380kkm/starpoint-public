// audience: internal
// # multiplayer-time-test
// 该测试验证房间生命周期使用注入的服务器时间, 而不是直接读取系统时间.

const assert = require("node:assert/strict")
const { MatchmakingStore, matchmakingStore } = require("../../out/multiplayer/matchmakingStore")
const { getServerDate, getServerTime, setServerTime, setServerTimeRate } = require("../../out/utils")

// //// 验证房间时钟和显式日期使用不同时间语义 [@x380kkm 2026-08-03] ////
function createRequest() {
    return {
        hostAccountId: 1,
        hostViewerId: 100000001,
        categoryId: 6,
        questId: 5001,
        partyId: 1,
    }
}

function testInjectedClockControlsRoomLifetime() {
    const clock = {
        now: 1_700_000_000_000,
        getCurrentTimeMilliseconds() {
            return this.now
        },
    }
    const store = new MatchmakingStore(clock)
    const room = store.createRoom(createRequest())
    assert.equal(room.createdAt, clock.now)
    assert.notEqual(store.getRoom(room.roomNumber), null)
    clock.now += 30 * 60 * 1000
    assert.equal(store.getRoom(room.roomNumber), null)
}

function testDefaultStoreUsesVirtualServerTime() {
    const virtualDate = new Date("2024-05-01T00:00:00.000Z")
    setServerTimeRate(1)
    setServerTime(virtualDate)
    try {
        const room = matchmakingStore.createRoom({ ...createRequest(), hostAccountId: 2, hostViewerId: 100000002 })
        assert.ok(Math.abs(room.createdAt - virtualDate.getTime()) < 100)
        assert.equal(matchmakingStore.getRoom(room.roomNumber)?.roomNumber, room.roomNumber)
    } finally {
        setServerTime(null)
    }
}

function testExplicitDatesRemainStableUnderVirtualTime() {
    const virtualDate = new Date("2024-05-01T00:00:00.000Z")
    const storedDate = new Date("2026-08-03T14:00:00.000Z")
    setServerTimeRate(1)
    setServerTime(virtualDate)
    try {
        assert.ok(Math.abs(getServerTime() - virtualDate.getTime() / 1000) < 1)
        assert.ok(Math.abs(getServerDate().getTime() - virtualDate.getTime()) < 1_000)
        assert.equal(getServerTime(storedDate), storedDate.getTime() / 1000)
    } finally {
        setServerTime(null)
    }
}

testInjectedClockControlsRoomLifetime()
testDefaultStoreUsesVirtualServerTime()
testExplicitDatesRemainStableUnderVirtualTime()
console.log("multiplayer virtual-time checks passed")
// //// /验证房间时钟和显式日期使用不同时间语义 ////
