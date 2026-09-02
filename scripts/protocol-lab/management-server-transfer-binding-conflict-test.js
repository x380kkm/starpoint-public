// audience: internal
// # management-server-transfer-binding-conflict-test
//
// 该脚本验证完整 Node 服务器绑定的 pull, 双分支冲突和三种解决策略.
// 两种覆盖策略都先验证对侧晚更新会被条件 ETag 拒绝.

const assert = require("node:assert/strict")
const {
    createBranchPackage,
    createServerTransferBindingFixture,
} = require("./management-server-transfer-binding-helpers")

async function synchronize(context, direction, expectedStatus = 200) {
    return context.requestInstance(context.sourceInstance, {
        method: "POST",
        path: `${context.bindingPath}/sync`,
        payload: { direction },
        expectedStatus,
    })
}

async function resolveConflict(context, conflictId, resolution, expectedStatus = 200) {
    return context.requestInstance(context.sourceInstance, {
        method: "POST",
        path: `${context.bindingPath}/conflicts/${conflictId}/resolve`,
        payload: { resolution },
        expectedStatus,
    })
}

async function removeBinding(context) {
    const removed = await context.requestInstance(context.sourceInstance, {
        method: "DELETE",
        path: context.bindingPath,
        expectedStatus: 200,
    })
    assert.equal(removed.body.deleted, true)
}

async function verifyManualPull(input) {
    const context = await createServerTransferBindingFixture(input, {
        label: "Server binding pull",
        uploadMode: "manual",
        pullMode: "manual",
        intervalSeconds: 900,
    })
    const baseline = await synchronize(context, "auto")
    assert.equal(baseline.body.action, "unchanged")

    const targetName = "Server binding pull target"
    const targetPackage = createBranchPackage(context.targetInitial.body, targetName)
    const targetBranch = await context.overwriteTransferSlot(
        context.targetTransfer,
        context.targetInitial.etag,
        targetPackage,
    )
    const pulled = await synchronize(context, "pull")
    assert.equal(pulled.body.action, "downloaded")
    assert.equal(pulled.body.binding.lastCommonEtag, targetBranch.body.etag)
    const sourceAfterPull = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterPull = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(sourceAfterPull.body.data.user_info.name, targetName)
    assert.equal(sourceAfterPull.body.payloadSha256, targetPackage.payloadSha256)
    assert.equal(targetAfterPull.etag, targetBranch.etag)
    assert.equal(targetAfterPull.body.payloadSha256, targetPackage.payloadSha256)
    await removeBinding(context)
}

async function verifyForcedDirections(input) {
    const context = await createServerTransferBindingFixture(input, {
        label: "Server binding forced directions",
        uploadMode: "manual",
        pullMode: "manual",
        intervalSeconds: 900,
    })
    const baseline = await synchronize(context, "auto")
    assert.equal(baseline.body.action, "unchanged")

    const targetBranchPackage = createBranchPackage(
        context.targetInitial.body,
        "Forced upload target branch",
    )
    await context.overwriteTransferSlot(
        context.targetTransfer,
        context.targetInitial.etag,
        targetBranchPackage,
    )
    const forcedUpload = await synchronize(context, "upload")
    assert.equal(forcedUpload.body.action, "uploaded")
    const targetAfterUpload = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(
        targetAfterUpload.body.payloadSha256,
        context.sourceInitial.body.payloadSha256,
    )

    const sourceAfterUpload = await context.exportTransferSlot(context.sourceTransfer)
    const sourceBranchPackage = createBranchPackage(
        sourceAfterUpload.body,
        "Forced pull source branch",
    )
    const sourceBranch = await context.overwriteTransferSlot(
        context.sourceTransfer,
        sourceAfterUpload.etag,
        sourceBranchPackage,
    )
    const forcedPull = await synchronize(context, "pull")
    assert.equal(forcedPull.body.action, "downloaded")
    const sourceAfterPull = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterPull = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(sourceAfterPull.body.payloadSha256, targetAfterPull.body.payloadSha256)
    assert.equal(sourceAfterPull.body.payloadSha256, context.sourceInitial.body.payloadSha256)
    const sourceRevisions = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `/manage/api/saves/${context.sourceSlot.playerId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(
        sourceRevisions.body.revisions.some(
            (revision) => revision.id === sourceBranch.body.revisionId,
        ),
        true,
    )
    await removeBinding(context)
}

async function createConflict(input, label) {
    const context = await createServerTransferBindingFixture(input, {
        label,
        uploadMode: "manual",
        pullMode: "manual",
        intervalSeconds: 900,
    })
    const baseline = await synchronize(context, "auto")
    assert.equal(baseline.body.action, "unchanged")
    const sourceName = `${label} source branch`
    const targetName = `${label} target branch`
    const sourcePackage = createBranchPackage(context.sourceInitial.body, sourceName)
    const targetPackage = createBranchPackage(context.targetInitial.body, targetName)
    const sourceBranch = await context.overwriteTransferSlot(
        context.sourceTransfer,
        context.sourceInitial.etag,
        sourcePackage,
    )
    const targetBranch = await context.overwriteTransferSlot(
        context.targetTransfer,
        context.targetInitial.etag,
        targetPackage,
    )
    const detected = await synchronize(context, "auto", 409)
    assert.equal(detected.body.error, "transfer_conflict")
    assert.equal(detected.body.conflict.sourceEtag, sourceBranch.body.etag)
    assert.equal(detected.body.conflict.targetEtag, targetBranch.body.etag)
    const conflicts = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `${context.bindingPath}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(conflicts.body.length, 1)
    assert.equal(conflicts.body[0].status, "open")
    return {
        ...context,
        conflictId: detected.body.conflict.conflictId,
        sourceName,
        targetName,
        sourcePackage,
        targetPackage,
        sourceBranch,
        targetBranch,
    }
}

async function verifyLocalWins(input) {
    const context = await createConflict(input, "Server local wins")
    const sourceAfterConflict = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterConflict = await context.exportTransferSlot(context.targetTransfer)
    const lateTargetName = "Server local wins late target"
    const lateTarget = await context.overwriteTransferSlot(
        context.targetTransfer,
        targetAfterConflict.etag,
        createBranchPackage(targetAfterConflict.body, lateTargetName),
    )
    const rejected = await resolveConflict(context, context.conflictId, "local_wins", 409)
    assert.equal(rejected.body.error, "conflict_changed")
    const targetAfterRejected = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(targetAfterRejected.body.data.user_info.name, lateTargetName)
    assert.equal(targetAfterRejected.etag, lateTarget.etag)
    const openConflicts = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `${context.bindingPath}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(openConflicts.body[0].status, "open")
    assert.equal(openConflicts.body[0].targetEtag, lateTarget.body.etag)

    const resolved = await resolveConflict(context, context.conflictId, "local_wins")
    assert.equal(resolved.body.conflict.status, "resolved_local_wins")
    const sourceAfterResolve = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterResolve = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(sourceAfterResolve.body.data.user_info.name, context.sourceName)
    assert.equal(targetAfterResolve.body.data.user_info.name, context.sourceName)
    assert.equal(sourceAfterResolve.etag, sourceAfterConflict.etag)
    assert.equal(targetAfterResolve.etag, sourceAfterConflict.etag)
    const targetRevisions = await context.requestInstance(context.targetInstance, {
        method: "GET",
        path: `/manage/api/saves/${context.targetSlot.playerId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(
        targetRevisions.body.revisions.some(
            (revision) => revision.id === context.targetBranch.body.revisionId,
        ),
        true,
    )
    await removeBinding(context)
}

async function verifyRemoteWins(input) {
    const context = await createConflict(input, "Server remote wins")
    const sourceAfterConflict = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterConflict = await context.exportTransferSlot(context.targetTransfer)
    const lateSourceName = "Server remote wins late source"
    const lateSource = await context.overwriteTransferSlot(
        context.sourceTransfer,
        sourceAfterConflict.etag,
        createBranchPackage(sourceAfterConflict.body, lateSourceName),
    )
    const rejected = await resolveConflict(context, context.conflictId, "remote_wins", 409)
    assert.equal(rejected.body.error, "conflict_changed")
    const sourceAfterRejected = await context.exportTransferSlot(context.sourceTransfer)
    assert.equal(sourceAfterRejected.body.data.user_info.name, lateSourceName)
    assert.equal(sourceAfterRejected.etag, lateSource.etag)
    const targetAfterRejected = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(targetAfterRejected.etag, targetAfterConflict.etag)
    const openConflicts = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `${context.bindingPath}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(openConflicts.body[0].status, "open")
    assert.equal(openConflicts.body[0].sourceEtag, lateSource.body.etag)
    assert.equal(openConflicts.body[0].targetEtag, targetAfterConflict.body.payloadSha256)

    const resolved = await resolveConflict(context, context.conflictId, "remote_wins")
    assert.equal(resolved.body.conflict.status, "resolved_remote_wins")
    const sourceAfterResolve = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterResolve = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(sourceAfterResolve.body.data.user_info.name, context.targetName)
    assert.equal(targetAfterResolve.body.data.user_info.name, context.targetName)
    assert.equal(sourceAfterResolve.etag, targetAfterConflict.etag)
    assert.equal(targetAfterResolve.etag, targetAfterConflict.etag)
    const sourceRevisions = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `/manage/api/saves/${context.sourceSlot.playerId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(
        sourceRevisions.body.revisions.some(
            (revision) => revision.id === context.sourceBranch.body.revisionId,
        ),
        true,
    )
    await removeBinding(context)
}

async function verifyRemoteWinsTargetRefresh(input) {
    const context = await createConflict(input, "Server remote wins target refresh")
    const sourceAfterConflict = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterConflict = await context.exportTransferSlot(context.targetTransfer)
    const lateTargetName = "Server remote wins newest target"
    const lateTarget = await context.overwriteTransferSlot(
        context.targetTransfer,
        targetAfterConflict.etag,
        createBranchPackage(targetAfterConflict.body, lateTargetName),
    )
    const rejected = await resolveConflict(context, context.conflictId, "remote_wins", 409)
    assert.equal(rejected.body.error, "conflict_changed")
    const openConflicts = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `${context.bindingPath}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(openConflicts.body[0].status, "open")
    assert.equal(openConflicts.body[0].sourceEtag, sourceAfterConflict.body.payloadSha256)
    assert.equal(openConflicts.body[0].targetEtag, lateTarget.body.etag)

    const resolved = await resolveConflict(context, context.conflictId, "remote_wins")
    assert.equal(resolved.body.conflict.status, "resolved_remote_wins")
    const sourceAfterResolve = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterResolve = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(sourceAfterResolve.body.data.user_info.name, lateTargetName)
    assert.equal(targetAfterResolve.body.data.user_info.name, lateTargetName)
    assert.equal(sourceAfterResolve.etag, lateTarget.etag)
    assert.equal(targetAfterResolve.etag, lateTarget.etag)
    await removeBinding(context)
}

async function verifyKeepBoth(input) {
    const context = await createConflict(input, "Server keep both")
    const sourceAfterConflict = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterConflict = await context.exportTransferSlot(context.targetTransfer)
    const resolved = await resolveConflict(context, context.conflictId, "keep_both")
    assert.equal(resolved.body.conflict.status, "resolved_keep_both")
    assert.equal(resolved.body.binding.enabled, false)
    const sourceAfterResolve = await context.exportTransferSlot(context.sourceTransfer)
    const targetAfterResolve = await context.exportTransferSlot(context.targetTransfer)
    assert.equal(sourceAfterResolve.etag, sourceAfterConflict.etag)
    assert.equal(targetAfterResolve.etag, targetAfterConflict.etag)
    assert.equal(sourceAfterResolve.body.payloadSha256, sourceAfterConflict.body.payloadSha256)
    assert.equal(targetAfterResolve.body.payloadSha256, targetAfterConflict.body.payloadSha256)
    const disabled = await synchronize(context, "auto", 409)
    assert.equal(disabled.body.error, "transfer_binding_disabled")
    await removeBinding(context)
}

async function verifyAutomaticPolicy(input, conflictPolicy, expectedAction) {
    const label = `Server automatic ${conflictPolicy}`
    const context = await createServerTransferBindingFixture(input, {
        label,
        uploadMode: "manual",
        pullMode: "manual",
        conflictPolicy,
        intervalSeconds: 900,
    })
    const baseline = await synchronize(context, "auto")
    assert.equal(baseline.body.action, "unchanged")
    const sourceName = `${label} source`
    const targetName = `${label} target`
    await context.overwriteTransferSlot(
        context.sourceTransfer,
        context.sourceInitial.etag,
        createBranchPackage(context.sourceInitial.body, sourceName),
    )
    await context.overwriteTransferSlot(
        context.targetTransfer,
        context.targetInitial.etag,
        createBranchPackage(context.targetInitial.body, targetName),
    )
    const synchronized = await synchronize(context, "auto")
    assert.equal(synchronized.body.action, expectedAction)
    const source = await context.exportTransferSlot(context.sourceTransfer)
    const target = await context.exportTransferSlot(context.targetTransfer)
    const expectedName = conflictPolicy === "local_wins" ? sourceName : targetName
    assert.equal(source.body.data.user_info.name, expectedName)
    assert.equal(target.body.data.user_info.name, expectedName)
    assert.equal(source.body.payloadSha256, target.body.payloadSha256)
    const conflicts = await context.requestInstance(context.sourceInstance, {
        method: "GET",
        path: `${context.bindingPath}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(conflicts.body.length, 0)
    await removeBinding(context)
}

// //// 验证服务器绑定的下载和冲突策略 [@x380kkm 2026-08-04] ////
async function verifyServerTransferBindingConflicts(input) {
    await verifyManualPull(input)
    await verifyForcedDirections(input)
    await verifyLocalWins(input)
    await verifyRemoteWins(input)
    await verifyRemoteWinsTargetRefresh(input)
    await verifyKeepBoth(input)
    await verifyAutomaticPolicy(input, "local_wins", "uploaded")
    await verifyAutomaticPolicy(input, "remote_wins", "downloaded")
}
// //// /验证服务器绑定的下载和冲突策略 ////

module.exports = { verifyServerTransferBindingConflicts }
