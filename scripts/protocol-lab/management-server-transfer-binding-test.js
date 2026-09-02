// audience: internal
// # management-server-transfer-binding-test
//
// 该脚本验证完整 Node 服务器持久化跨实例绑定并自动上传到另一个 Node 实例.
// 测试在来源服务器重启后复用同一绑定和远端槽 token.

const assert = require("node:assert/strict")
const {
    createBranchPackage,
    createServerTransferBindingFixture,
} = require("./management-server-transfer-binding-helpers")

const waitTimeoutMilliseconds = 15_000
const waitIntervalMilliseconds = 200

// //// 验证同一 binding 的远端操作互斥 [@x380kkm 2026-08-04] ////
async function verifyServerTransferOperationLock() {
    const {
        ServerTransferBindingOperationError,
        isServerTransferBindingBusy,
        runExclusiveServerTransferOperation,
    } = require("../../out/control/serverTransferBindingOperations")
    let releaseOperation
    const gate = new Promise((resolve) => {
        releaseOperation = resolve
    })
    const active = runExclusiveServerTransferOperation("test-binding", async () => gate)
    assert.equal(isServerTransferBindingBusy("test-binding"), true)
    await assert.rejects(
        runExclusiveServerTransferOperation("test-binding", async () => undefined),
        (error) => error instanceof ServerTransferBindingOperationError
            && error.code === "transfer_binding_busy",
    )
    releaseOperation()
    await active
    assert.equal(isServerTransferBindingBusy("test-binding"), false)
}
// //// /验证同一 binding 的远端操作互斥 ////

async function waitFor(check, label) {
    const deadline = Date.now() + waitTimeoutMilliseconds
    let lastError = null
    while (Date.now() < deadline) {
        try {
            const value = await check()
            if (value !== null) return value
        } catch (error) {
            lastError = error
        }
        await new Promise((resolve) => setTimeout(resolve, waitIntervalMilliseconds))
    }
    if (lastError !== null) throw lastError
    throw new Error(`${label} did not complete before timeout.`)
}

async function waitForBindingBaseline(context) {
    return waitFor(async () => {
        const response = await context.requestInstance(context.sourceInstance, {
            method: "GET",
            path: context.bindingPath,
            expectedStatus: 200,
        })
        return response.body.lastCommonEtag === context.initialEtag ? response.body : null
    }, "Server transfer binding baseline")
}

async function verifyUnchangedRunnerKeepsRevisionCount(context) {
    const revisionsPath = `/manage/api/saves/${context.sourceSlot.playerId}/revisions`
    const before = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: revisionsPath,
        expectedStatus: 200,
    })
    await new Promise((resolve) => setTimeout(resolve, 2_200))
    const after = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: revisionsPath,
        expectedStatus: 200,
    })
    assert.deepEqual(
        after.body.revisions.map((revision) => revision.id),
        before.body.revisions.map((revision) => revision.id),
    )
}

async function waitForTargetBranch(context, name, payloadSha256) {
    try {
        return await waitFor(async () => {
            const current = await context.exportTransferSlot(context.targetTransfer)
            if (
                current.body.data.user_info.name !== name
                || current.body.payloadSha256 !== payloadSha256
            ) {
                return null
            }
            return current
        }, `Server transfer target branch ${name}`)
    } catch (error) {
        const binding = await context.requestInstance(context.sourceInstance, {
            method: "GET",
            path: context.bindingPath,
            expectedStatus: 200,
        })
        const target = await context.exportTransferSlot(context.targetTransfer)
        throw new Error(`${error.message} ${JSON.stringify({
            lastError: binding.body.lastError,
            pendingDirection: binding.body.pendingDirection,
            lastCommonEtag: binding.body.lastCommonEtag,
            lastSourceEtag: binding.body.lastSourceEtag,
            lastTargetEtag: binding.body.lastTargetEtag,
            targetEtag: target.body.payloadSha256,
        })}`)
    }
}

// //// 创建并运行服务器到服务器的 interval 绑定 [@x380kkm 2026-08-04] ////
async function prepareServerTransferBinding(input) {
    await verifyServerTransferOperationLock()
    const context = await createServerTransferBindingFixture(input, {
        label: "Server binding",
        uploadMode: "interval",
        pullMode: "manual",
        intervalSeconds: 1,
    })
    await waitForBindingBaseline(context)
    await verifyUnchangedRunnerKeepsRevisionCount(context)

    const firstName = "Server binding before restart"
    const firstPackage = createBranchPackage(context.sourceInitial.body, firstName)
    await input.overwriteTransferSlot(
        context.sourceTransfer,
        context.sourceInitial.etag,
        firstPackage,
    )
    await waitForTargetBranch(context, firstName, firstPackage.payloadSha256)
    return context
}
// //// /创建并运行服务器到服务器的 interval 绑定 ////

// //// 验证来源服务器重启后继续自动上传 [@x380kkm 2026-08-04] ////
async function verifyServerTransferBindingAfterRestart(context, sourceInstance) {
    context.sourceInstance = sourceInstance
    context.sourceTransfer.baseUrl = sourceInstance.baseUrl
    const persisted = await context.requestInstance(sourceInstance, {
        method: "GET",
        path: context.bindingPath,
        expectedStatus: 200,
    })
    assert.equal(persisted.body.enabled, true)
    assert.equal(persisted.body.target.instanceId, context.targetTransfer.instanceId)
    assert.equal(JSON.stringify(persisted.body).includes(context.targetTransfer.token), false)

    const sourceCurrent = await context.exportTransferSlot(context.sourceTransfer)
    const secondName = "Server binding after restart"
    const secondPackage = createBranchPackage(sourceCurrent.body, secondName)
    await context.overwriteTransferSlot(
        context.sourceTransfer,
        sourceCurrent.etag,
        secondPackage,
    )
    await waitForTargetBranch(context, secondName, secondPackage.payloadSha256)

    const removed = await context.requestInstance(sourceInstance, {
        method: "DELETE",
        path: context.bindingPath,
        expectedStatus: 200,
    })
    assert.equal(removed.body.deleted, true)
}
// //// /验证来源服务器重启后继续自动上传 ////

module.exports = {
    prepareServerTransferBinding,
    verifyServerTransferBindingAfterRestart,
}
