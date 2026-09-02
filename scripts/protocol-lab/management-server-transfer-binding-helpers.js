// audience: internal
// # management-server-transfer-binding-helpers
//
// 此模块为完整服务器绑定验收创建隔离的来源槽, 目标槽和绑定.
// 测试断言所有管理响应都不包含远端槽 token.

const assert = require("node:assert/strict")
const {
    createStarpointSavePackage,
} = require("../../out/games/starpoint/portableSave")

function createBranchPackage(sourcePackage, name) {
    const data = structuredClone(sourcePackage.data)
    data.user_info.name = name
    return createStarpointSavePackage({
        data,
        createdAt: new Date().toISOString(),
        source: sourcePackage.source,
        sourceClient: sourcePackage.sourceClient,
    })
}

// //// 创建隔离的服务器绑定测试数据 [@x380kkm 2026-08-04] ////
async function createServerTransferBindingFixture(input, config) {
    const sourceSlot = await input.importNodeSlot(input.sourceInstance, input.initialPackage)
    const targetSlot = await input.importNodeSlot(input.targetInstance, input.initialPackage)
    const sourceTransfer = await input.issueNodeTransferAccess(
        input.sourceInstance,
        sourceSlot.playerId,
        `${config.label} source`,
    )
    const targetTransfer = await input.issueNodeTransferAccess(
        input.targetInstance,
        targetSlot.playerId,
        `${config.label} target`,
    )
    const sourceInitial = await input.exportTransferSlot(sourceTransfer)
    const targetInitial = await input.exportTransferSlot(targetTransfer)
    assert.equal(sourceInitial.body.payloadSha256, targetInitial.body.payloadSha256)

    const created = await input.requestInstance(input.sourceInstance, {
        method: "POST",
        path: `/manage/api/saves/${sourceSlot.playerId}/transfer-bindings`,
        payload: {
            targetBaseUrl: `${input.targetInstance.baseUrl}/manage/transfer/v1`,
            targetInstanceId: targetTransfer.instanceId,
            targetPlayerId: targetTransfer.slotId,
            targetToken: targetTransfer.token,
            uploadMode: config.uploadMode,
            pullMode: config.pullMode,
            conflictPolicy: config.conflictPolicy ?? "ask",
            intervalSeconds: config.intervalSeconds,
            enabled: true,
        },
        expectedStatus: 201,
    })
    assert.equal(JSON.stringify(created.body).includes(targetTransfer.token), false)
    assert.equal(Object.hasOwn(created.body.target, "token"), false)
    assert.equal(created.body.target.instanceId, targetTransfer.instanceId)
    assert.equal(created.body.target.playerId, targetTransfer.slotId)
    const bindingPath = `/manage/api/saves/${sourceSlot.playerId}/transfer-bindings/${created.body.bindingId}`
    return {
        ...input,
        sourceSlot,
        targetSlot,
        sourceTransfer,
        targetTransfer,
        sourceInitial,
        targetInitial,
        bindingPath,
        initialEtag: sourceInitial.body.payloadSha256,
    }
}
// //// /创建隔离的服务器绑定测试数据 ////

module.exports = {
    createBranchPackage,
    createServerTransferBindingFixture,
}
