// audience: internal
// # management-three-instance-portable-save-test
//
// 该脚本用 Rust 本地实例 L 和两个隔离 Node 服务器 A/B 验证可移植存档.
// 测试覆盖实例身份隔离, 双向搬运, 三种冲突解决, revision 恢复和远端停止后的本地读取.
// 既有槽的跨实例读写使用槽 token, 新槽创建使用各实例管理接口.

const assert = require("node:assert/strict")
const path = require("node:path")
const {
    createStarpointSavePackage,
} = require("../../out/games/starpoint/portableSave.js")
const {
    loadCn,
    requestJson,
    signupCn,
    startNodeServer,
    startPersonalService,
    stopChildProcess,
} = require("./loopback-test-services")
const {
    prepareServerTransferBinding,
    verifyServerTransferBindingAfterRestart,
} = require("./management-server-transfer-binding-test")
const {
    verifyServerTransferBindingConflicts,
} = require("./management-server-transfer-binding-conflict-test")
const {
    verifyServerTransferBindingRaces,
} = require("./management-server-transfer-binding-race-test")

const nodeBManagementToken = "three-instance-node-b-management-token"
const nodeTransferPrefix = "/manage/transfer/v1"
const localTransferPrefix = "/v1/transfer/v1"

// //// 验证三个实例之间的可移植存档闭环 [@x380kkm 2026-08-03] ////
async function verifyThreeInstancePortableSave(app, root, nodeAContext) {
    const address = app.server.address()
    assert.notEqual(address, null)
    assert.equal(typeof address, "object")
    assert.equal(address.address, "127.0.0.1")
    const nodeA = {
        baseUrl: `http://127.0.0.1:${address.port}`,
        managementToken: nodeAContext.managementToken,
        targetPlayerId: nodeAContext.playerId,
        viewerId: String(nodeAContext.viewerId),
    }
    const nodeBRoot = path.join(root, "three-instance-node-b")
    let nodeB = null
    let personalService = null
    let nodeBStopped = true
    try {
        nodeB = await startNodeServer(nodeBRoot, nodeBManagementToken)
        nodeBStopped = false
        personalService = await startPersonalService(path.join(root, "three-instance-local-l"))
        const local = await prepareLocalInstance(personalService)
        const nodeBContext = await prepareNodeBInstance(nodeB)
        Object.assign(nodeB, nodeBContext)

        const nodeAImported = await importAndActivateNodeSlot(nodeA, local.package)
        const nodeBImported = await importAndActivateNodeSlot(nodeB, local.package)
        const nodeATransfer = await issueNodeTransferAccess(nodeA, nodeAImported.playerId, "Server A")
        const nodeBTransfer = await issueNodeTransferAccess(nodeB, nodeBImported.playerId, "Server B")
        const serverBindingTestInput = {
            sourceInstance: nodeB,
            targetInstance: nodeA,
            initialPackage: local.package,
            importNodeSlot,
            issueNodeTransferAccess,
            requestInstance,
            overwriteTransferSlot,
            exportTransferSlot,
        }
        const serverTransferBinding = await prepareServerTransferBinding(serverBindingTestInput)
        await verifyServerTransferBindingConflicts(serverBindingTestInput)
        await verifyServerTransferBindingRaces(serverBindingTestInput)
        const localInitial = await exportTransferSlot(local.transfer)
        let nodeAInitial = await exportTransferSlot(nodeATransfer)
        let nodeBInitial = await exportTransferSlot(nodeBTransfer)
        assert.deepEqual(localInitial.body.data, local.package.data)
        assert.equal(localInitial.body.payloadSha256, local.package.payloadSha256)
        assert.deepEqual(nodeAInitial.body.data, local.package.data)
        assert.deepEqual(nodeBInitial.body.data, local.package.data)
        assert.equal(nodeAInitial.body.payloadSha256, local.package.payloadSha256)
        assert.equal(nodeBInitial.body.payloadSha256, local.package.payloadSha256)
        await Promise.all([
            overwriteTransferSlot(nodeATransfer, nodeAInitial.etag, localInitial.body),
            overwriteTransferSlot(nodeBTransfer, nodeBInitial.etag, localInitial.body),
        ])
        const refreshedInitial = await Promise.all([
            exportTransferSlot(nodeATransfer),
            exportTransferSlot(nodeBTransfer),
        ])
        nodeAInitial = refreshedInitial[0]
        nodeBInitial = refreshedInitial[1]
        assert.deepEqual(nodeAInitial.body.data, localInitial.body.data)
        assert.deepEqual(nodeBInitial.body.data, localInitial.body.data)
        assert.equal(nodeAInitial.body.payloadSha256, localInitial.body.payloadSha256)
        assert.equal(nodeBInitial.body.payloadSha256, localInitial.body.payloadSha256)

        assert.equal(new Set([
            local.instanceId,
            nodeATransfer.instanceId,
            nodeBTransfer.instanceId,
        ]).size, 3)
        assert.equal(new Set([
            String(local.viewerId),
            nodeA.viewerId,
            String(nodeB.viewerId),
        ]).size, 3)
        await verifySlotTokenCannotAccessAnotherSlot(nodeATransfer, nodeA.targetPlayerId)
        await verifyTransferBindingConflict(personalService, local, nodeA, local.package)

        const branches = await createIndependentNodeBranches(
            nodeATransfer,
            nodeAInitial,
            nodeBTransfer,
            nodeBInitial,
        )
        await verifyStaleNodeUploadIsRejected(
            nodeA,
            nodeATransfer,
            nodeAInitial,
            branches.nodeA,
            local.package,
        )
        await verifyNodeToNodeTransferAndRestore(
            nodeATransfer,
            nodeB,
            nodeBTransfer,
            branches,
        )
        const uploadedToLocal = await overwriteTransferSlot(
            local.transfer,
            localInitial.etag,
            branches.nodeA.export.body,
        )
        assert.equal(uploadedToLocal.body.imported, true)

        const nodeBCopy = await importAndActivateNodeSlot(nodeB, branches.nodeA.export.body)
        const nodeBLoad = await loadCn(nodeB.baseUrl, nodeB.viewerId, 93002)
        assert.equal(String(nodeBLoad.data_headers.viewer_id), String(nodeB.viewerId))
        assert.equal(nodeBLoad.data.user_info.name, "Three instance branch A")
        const nodeBCopyIdentity = await issueNodeShellToken(nodeB, nodeBCopy.playerId, "Server B copy")
        assert.equal(nodeBCopyIdentity.instanceId, nodeBTransfer.instanceId)
        const nodeBSlots = await requestInstance(nodeB, {
            method: "GET",
            path: "/manage/api/saves",
            expectedStatus: 200,
        })
        assert.equal(nodeBSlots.body.players.some((player) => player.id === nodeBImported.playerId), true)
        assert.equal(nodeBSlots.body.players.some((player) => player.id === nodeBCopy.playerId), true)
        const nodeBBeforeRestart = await exportNodeSlot(nodeB, nodeBCopy.playerId)

        const localImport = await requestInstance(personalService, {
            method: "POST",
            path: "/v1/local-saves/import",
            payload: branches.nodeA.export.body,
            expectedStatus: 201,
        })
        const localImported = await requestInstance(personalService, {
            method: "GET",
            path: `/v1/local-saves/${localImport.body.id}/export`,
            expectedStatus: 200,
        })
        assert.deepEqual(localImported.body.data, branches.nodeA.export.body.data)
        assert.equal(localImported.body.payloadSha256, branches.nodeA.export.body.payloadSha256)

        const nodeBViewerId = nodeB.viewerId
        await stopChildProcess(nodeB.process, "Node server B")
        nodeBStopped = true
        nodeB = await startNodeServer(nodeBRoot, nodeBManagementToken)
        nodeBStopped = false
        nodeB.viewerId = nodeBViewerId
        nodeBTransfer.baseUrl = nodeB.baseUrl
        const restartedNodeBExport = await exportNodeSlot(nodeB, nodeBCopy.playerId)
        assert.deepEqual(restartedNodeBExport.body.data, nodeBBeforeRestart.body.data)
        assert.equal(restartedNodeBExport.body.payloadSha256, nodeBBeforeRestart.body.payloadSha256)
        const restartedNodeBTransfer = await exportTransferSlot(nodeBTransfer)
        assert.deepEqual(restartedNodeBTransfer.body.data, branches.nodeB.export.body.data)
        assert.equal(restartedNodeBTransfer.body.payloadSha256, branches.nodeB.export.body.payloadSha256)
        const restartedNodeBIdentity = await issueNodeShellToken(nodeB, nodeBCopy.playerId, "Server B restart")
        assert.equal(restartedNodeBIdentity.instanceId, nodeBTransfer.instanceId)
        const restartedNodeBLoad = await loadCn(nodeB.baseUrl, nodeB.viewerId, 93002)
        assert.equal(restartedNodeBLoad.data.user_info.name, "Three instance branch A")
        await verifyServerTransferBindingAfterRestart(serverTransferBinding, nodeB)
        await stopChildProcess(nodeB.process, "Node server B")
        nodeBStopped = true
        const localAfterRemoteStop = await requestInstance(personalService, {
            method: "GET",
            path: `/v1/local-saves/${local.slotId}/export`,
            expectedStatus: 200,
        })
        assert.deepEqual(localAfterRemoteStop.body.data, branches.nodeA.export.body.data)
    } finally {
        await stopThreeInstanceProcesses(nodeB, nodeBStopped, personalService)
    }
}

async function stopThreeInstanceProcesses(nodeB, nodeBStopped, personalService) {
    const stops = []
    if (nodeB !== null && !nodeBStopped) stops.push(stopChildProcess(nodeB.process, "Node server B"))
    if (personalService !== null) stops.push(stopChildProcess(personalService.process, "Personal service probe"))
    const errors = (await Promise.allSettled(stops))
        .filter((result) => result.status === "rejected")
        .map((result) => result.reason)
    if (errors.length > 0) throw new AggregateError(errors, "Three instance test processes did not stop.")
}
// //// /验证三个实例之间的可移植存档闭环 ////

// //// 验证明确绑定和双分支冲突队列 [@x380kkm 2026-08-03] ////
async function verifyTransferBindingConflict(personalService, local, nodeA, initialPackage) {
    const scenarios = [
        {
            label: "Local wins",
            localBranchName: "Transfer binding local branch",
            remoteBranchName: "Transfer binding remote branch",
            verifyResolution: verifyLocalWinsResolution,
        },
        {
            label: "Remote wins",
            localBranchName: "Remote wins local branch",
            remoteBranchName: "Remote wins remote branch",
            verifyResolution: verifyRemoteWinsResolution,
        },
        {
            label: "Keep both",
            localBranchName: "Keep both local branch",
            remoteBranchName: "Keep both remote branch",
            verifyResolution: verifyKeepBothResolution,
        },
    ]
    for (const scenario of scenarios) {
        await verifyTransferBindingConflictScenario(
            personalService,
            local,
            nodeA,
            initialPackage,
            scenario,
        )
    }
}

async function verifyTransferBindingConflictScenario(
    personalService,
    local,
    nodeA,
    initialPackage,
    scenario,
) {
    const localCopy = await requestInstance(personalService, {
        method: "POST",
        path: `/v1/local-saves/${local.slotId}/copy`,
        payload: { name: `Transfer binding ${scenario.label} source` },
        expectedStatus: 201,
    })
    const localTransfer = await issueLocalTransferAccess(
        personalService,
        localCopy.body.id,
        `Transfer binding ${scenario.label} source`,
    )
    const remoteCopy = await importNodeSlot(nodeA, initialPackage)
    const remoteTransfer = await issueNodeTransferAccess(
        nodeA,
        remoteCopy.playerId,
        `Transfer binding ${scenario.label} target`,
    )
    const targetUrl = new URL(nodeA.baseUrl)
    const profile = await requestInstance(personalService, {
        method: "POST",
        path: "/v1/server-profiles",
        payload: {
            name: `Transfer binding ${scenario.label} server A`,
            scheme: targetUrl.protocol.slice(0, -1),
            host: targetUrl.hostname,
            port: Number(targetUrl.port),
        },
        expectedStatus: 201,
    })
    const bindingPath = `/v1/local-saves/${localCopy.body.id}/transfer-bindings`
    const created = await requestInstance(personalService, {
        method: "POST",
        path: bindingPath,
        payload: {
            target_profile_id: profile.body.id,
            target_instance_kind: "remote",
            target_instance_id: remoteTransfer.instanceId,
            target_slot_id: remoteTransfer.slotId,
            target_token: remoteTransfer.token,
            upload_mode: "manual",
            pull_mode: "manual",
            conflict_policy: "ask",
            interval_seconds: 900,
            enabled: true,
        },
        expectedStatus: 201,
    })
    assert.equal(JSON.stringify(created.body).includes(remoteTransfer.token), false)
    assert.equal(Object.hasOwn(created.body.target, "token"), false)
    assert.equal(created.body.source.slot_id, localCopy.body.id)
    assert.equal(created.body.target.instance_id, remoteTransfer.instanceId)
    const bindingId = created.body.binding_id
    const listedBindings = await requestInstance(personalService, {
        method: "GET",
        path: bindingPath,
        expectedStatus: 200,
    })
    assert.equal(JSON.stringify(listedBindings.body).includes(remoteTransfer.token), false)
    assert.equal(Object.hasOwn(listedBindings.body[0].target, "token"), false)

    const initialSync = await requestInstance(personalService, {
        method: "POST",
        path: `${bindingPath}/${bindingId}/sync`,
        expectedStatus: 200,
    })
    assert.equal(initialSync.body.action, "unchanged")

    const localInitial = await exportTransferSlot(localTransfer)
    const remoteInitial = await exportTransferSlot(remoteTransfer)
    assert.equal(initialSync.body.binding.last_common_etag, localInitial.body.payloadSha256)
    assert.equal(remoteInitial.body.payloadSha256, localInitial.body.payloadSha256)
    assert.equal(localInitial.instanceId, localTransfer.instanceId)
    assert.equal(remoteInitial.instanceId, remoteTransfer.instanceId)
    assert.match(localInitial.shellId, /^\d+$/)
    assert.equal(remoteInitial.shellId, remoteTransfer.shellId)
    const localBranch = await overwriteTransferSlot(
        localTransfer,
        localInitial.etag,
        createBranchPackage(localInitial.body, scenario.localBranchName),
    )
    const remoteBranchPackage = createBranchPackage(
        remoteInitial.body,
        scenario.remoteBranchName,
    )
    const remoteBranch = await overwriteTransferSlot(
        remoteTransfer,
        remoteInitial.etag,
        remoteBranchPackage,
    )
    const conflict = await requestInstance(personalService, {
        method: "POST",
        path: `${bindingPath}/${bindingId}/sync`,
        expectedStatus: 409,
    })
    assert.equal(conflict.body.error, "transfer_conflict")
    assert.equal(conflict.body.conflict.source_etag, localBranch.body.etag)
    assert.equal(conflict.body.conflict.target_etag, remoteBranch.body.etag)
    const localAfterConflict = await exportTransferSlot(localTransfer)
    const remoteAfterConflict = await exportTransferSlot(remoteTransfer)
    assert.equal(localAfterConflict.body.data.user_info.name, scenario.localBranchName)
    assert.equal(remoteAfterConflict.body.data.user_info.name, scenario.remoteBranchName)
    const conflicts = await requestInstance(personalService, {
        method: "GET",
        path: `${bindingPath}/${bindingId}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(conflicts.body.length, 1)
    assert.equal(conflicts.body[0].status, "open")

    await scenario.verifyResolution({
        personalService,
        nodeA,
        scenario,
        bindingPath,
        bindingId,
        conflict: conflict.body.conflict,
        initialCommonEtag: initialSync.body.binding.last_common_etag,
        localTransfer,
        remoteTransfer,
        localBranch,
        remoteBranch,
        remoteAfterConflict,
        remoteBranchPackage,
        localAfterConflict,
        remoteAfterConflict,
    })
}

async function verifyLocalWinsResolution(context) {
    const {
        personalService,
        nodeA,
        scenario,
        bindingPath,
        bindingId,
        conflict,
        localTransfer,
        remoteTransfer,
        localBranch,
        remoteBranch,
        remoteBranchPackage,
        localAfterConflict,
        remoteAfterConflict,
    } = context
    const lateRemoteBranchName = `${scenario.remoteBranchName} late`
    const lateRemoteBranch = await overwriteTransferSlot(
        remoteTransfer,
        remoteAfterConflict.etag,
        createBranchPackage(remoteAfterConflict.body, lateRemoteBranchName),
    )
    const rejectedResolution = await requestInstance(personalService, {
        method: "POST",
        path: conflictResolutionPath(context),
        payload: { resolution: "local_wins" },
        expectedStatus: 409,
    })
    assert.equal(rejectedResolution.body.error, "transfer_target_revision_conflict")
    const lateRemoteCurrent = await exportTransferSlot(remoteTransfer)
    assert.equal(lateRemoteCurrent.body.data.user_info.name, lateRemoteBranchName)
    const openAfterRejectedResolution = await requestInstance(personalService, {
        method: "GET",
        path: `${bindingPath}/${bindingId}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(openAfterRejectedResolution.body[0].status, "open")
    const restoredRemoteBranch = await overwriteTransferSlot(
        remoteTransfer,
        lateRemoteBranch.body.etag,
        remoteBranchPackage,
    )
    assert.equal(restoredRemoteBranch.body.etag, remoteBranch.body.etag)
    const resolved = await requestInstance(personalService, {
        method: "POST",
        path: conflictResolutionPath(context),
        payload: { resolution: "local_wins" },
        expectedStatus: 200,
    })
    assert.equal(resolved.body.conflict.status, "resolved_local_wins")
    assert.equal(resolved.body.binding.last_common_etag, localBranch.body.etag)
    const localAfterResolve = await exportTransferSlot(localTransfer)
    const remoteAfterResolve = await exportTransferSlot(remoteTransfer)
    assert.equal(localAfterResolve.body.data.user_info.name, scenario.localBranchName)
    assert.equal(remoteAfterResolve.body.data.user_info.name, scenario.localBranchName)
    assert.equal(localAfterResolve.etag, localAfterConflict.etag)
    assert.equal(remoteAfterResolve.etag, localAfterConflict.etag)
    assert.equal(localAfterResolve.body.payloadSha256, localAfterConflict.body.payloadSha256)
    assert.equal(remoteAfterResolve.body.payloadSha256, localAfterConflict.body.payloadSha256)
    const revisions = await requestInstance(nodeA, {
        method: "GET",
        path: `/manage/api/saves/${remoteTransfer.slotId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(
        revisions.body.revisions.some((revision) => revision.id === remoteBranch.body.revisionId),
        true,
    )

    const disabled = await requestInstance(personalService, {
        method: "PUT",
        path: `${bindingPath}/${bindingId}`,
        payload: {
            upload_mode: "manual",
            pull_mode: "manual",
            conflict_policy: "ask",
            interval_seconds: 900,
            enabled: false,
        },
        expectedStatus: 200,
    })
    assert.equal(disabled.body.enabled, false)
    const disabledSync = await requestInstance(personalService, {
        method: "POST",
        path: `${bindingPath}/${bindingId}/sync`,
        expectedStatus: 409,
    })
    assert.equal(disabledSync.body.error, "transfer_binding_disabled")
    const manualPackage = createBranchPackage(
        localAfterConflict.body,
        "Manual transfer after binding disabled",
    )
    const manualUpload = await overwriteTransferSlot(
        remoteTransfer,
        remoteAfterResolve.etag,
        manualPackage,
    )
    assert.equal(manualUpload.body.imported, true)
}

async function verifyRemoteWinsResolution(context) {
    const {
        personalService,
        scenario,
        bindingPath,
        bindingId,
        localTransfer,
        remoteTransfer,
        localBranch,
        remoteBranch,
        localAfterConflict,
        remoteAfterConflict,
    } = context
    const lateLocalBranchName = `${scenario.localBranchName} late`
    const lateLocalBranch = await overwriteTransferSlot(
        localTransfer,
        localAfterConflict.etag,
        createBranchPackage(localAfterConflict.body, lateLocalBranchName),
    )
    const rejectedResolution = await requestInstance(personalService, {
        method: "POST",
        path: conflictResolutionPath(context),
        payload: { resolution: "remote_wins" },
        expectedStatus: 409,
    })
    assert.equal(rejectedResolution.body.error, "transfer_conflict_changed")
    const lateLocalCurrent = await exportTransferSlot(localTransfer)
    assert.equal(lateLocalCurrent.body.data.user_info.name, lateLocalBranchName)
    assert.equal(lateLocalCurrent.etag, lateLocalBranch.etag)
    assert.equal(lateLocalCurrent.body.payloadSha256, lateLocalBranch.body.etag)
    const remoteBeforeRetry = await exportTransferSlot(remoteTransfer)
    assert.equal(remoteBeforeRetry.etag, remoteAfterConflict.etag)
    assert.equal(remoteBeforeRetry.body.payloadSha256, remoteAfterConflict.body.payloadSha256)
    const openAfterRejectedResolution = await requestInstance(personalService, {
        method: "GET",
        path: `${bindingPath}/${bindingId}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(openAfterRejectedResolution.body[0].status, "open")
    const restoredLocalBranch = await requestInstance(personalService, {
        method: "POST",
        path: `/v1/local-saves/${localTransfer.slotId}/revisions/${localBranch.body.revisionId}/restore`,
        headers: { "if-match": lateLocalCurrent.etag },
        expectedStatus: 200,
    })
    assert.equal(restoredLocalBranch.body.restored, true)
    const localBeforeRetry = await exportTransferSlot(localTransfer)
    assert.equal(localBeforeRetry.body.data.user_info.name, scenario.localBranchName)
    assert.equal(localBeforeRetry.etag, `"${localBranch.body.etag}"`)
    assert.equal(localBeforeRetry.body.payloadSha256, localBranch.body.etag)
    const resolved = await requestInstance(personalService, {
        method: "POST",
        path: conflictResolutionPath(context),
        payload: { resolution: "remote_wins" },
        expectedStatus: 200,
    })
    assert.equal(resolved.body.conflict.status, "resolved_remote_wins")
    assert.equal(resolved.body.binding.last_common_etag, remoteBranch.body.etag)
    const localAfterResolve = await exportTransferSlot(localTransfer)
    const remoteAfterResolve = await exportTransferSlot(remoteTransfer)
    assert.equal(localAfterResolve.body.data.user_info.name, scenario.remoteBranchName)
    assert.equal(remoteAfterResolve.body.data.user_info.name, scenario.remoteBranchName)
    assert.equal(localAfterResolve.etag, remoteAfterConflict.etag)
    assert.equal(remoteAfterResolve.etag, remoteAfterConflict.etag)
    assert.equal(localAfterResolve.body.payloadSha256, remoteAfterConflict.body.payloadSha256)
    assert.equal(remoteAfterResolve.body.payloadSha256, remoteAfterConflict.body.payloadSha256)
    const conflicts = await requestInstance(personalService, {
        method: "GET",
        path: `${bindingPath}/${bindingId}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(conflicts.body[0].status, "resolved_remote_wins")
    const revisions = await requestInstance(personalService, {
        method: "GET",
        path: `/v1/local-saves/${localTransfer.slotId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(
        revisions.body.revisions.some((revision) => revision.id === localBranch.body.revisionId),
        true,
    )
    const restored = await requestInstance(personalService, {
        method: "POST",
        path: `/v1/local-saves/${localTransfer.slotId}/revisions/${localBranch.body.revisionId}/restore`,
        headers: { "if-match": localAfterResolve.etag },
        expectedStatus: 200,
    })
    assert.equal(restored.body.restored, true)
    const localAfterRestore = await exportTransferSlot(localTransfer)
    assert.equal(localAfterRestore.body.data.user_info.name, scenario.localBranchName)
    assert.equal(localAfterRestore.etag, `"${localBranch.body.etag}"`)
    assert.equal(localAfterRestore.body.payloadSha256, localBranch.body.etag)
}

async function verifyKeepBothResolution(context) {
    const {
        personalService,
        scenario,
        bindingPath,
        bindingId,
        initialCommonEtag,
        localTransfer,
        remoteTransfer,
        localAfterConflict,
        remoteAfterConflict,
    } = context
    const resolved = await requestInstance(personalService, {
        method: "POST",
        path: conflictResolutionPath(context),
        payload: { resolution: "keep_both" },
        expectedStatus: 200,
    })
    assert.equal(resolved.body.conflict.status, "resolved_keep_both")
    assert.equal(resolved.body.binding.enabled, false)
    assert.equal(resolved.body.binding.last_common_etag, initialCommonEtag)
    const localAfterResolve = await exportTransferSlot(localTransfer)
    const remoteAfterResolve = await exportTransferSlot(remoteTransfer)
    assert.equal(localAfterResolve.body.data.user_info.name, scenario.localBranchName)
    assert.equal(remoteAfterResolve.body.data.user_info.name, scenario.remoteBranchName)
    assert.equal(localAfterResolve.etag, localAfterConflict.etag)
    assert.equal(remoteAfterResolve.etag, remoteAfterConflict.etag)
    assert.equal(localAfterResolve.body.payloadSha256, localAfterConflict.body.payloadSha256)
    assert.equal(remoteAfterResolve.body.payloadSha256, remoteAfterConflict.body.payloadSha256)
    const conflicts = await requestInstance(personalService, {
        method: "GET",
        path: `${bindingPath}/${bindingId}/conflicts`,
        expectedStatus: 200,
    })
    assert.equal(conflicts.body[0].status, "resolved_keep_both")
    const disabledSync = await requestInstance(personalService, {
        method: "POST",
        path: `${bindingPath}/${bindingId}/sync`,
        expectedStatus: 409,
    })
    assert.equal(disabledSync.body.error, "transfer_binding_disabled")
}

function conflictResolutionPath({ bindingPath, bindingId, conflict }) {
    return `${bindingPath}/${bindingId}/conflicts/${conflict.conflict_id}/resolve`
}
// //// /验证明确绑定和双分支冲突队列 ////

// //// 准备实例身份和初始槽 [@x380kkm 2026-08-03] ////
async function prepareLocalInstance(personalService) {
    const signup = await signupCn(personalService.baseUrl, 93001, "three-instance-local-l")
    const state = await requestInstance(personalService, {
        method: "GET",
        path: "/v1/local-saves",
        expectedStatus: 200,
    })
    assert.equal(state.body.slots.length, 1)
    const slotId = state.body.slots[0].id
    const exported = await requestInstance(personalService, {
        method: "GET",
        path: `/v1/local-saves/${slotId}/export`,
        expectedStatus: 200,
    })
    assert.equal(Object.hasOwn(exported.body.data, "associate_token"), false)
    assert.equal(Object.hasOwn(exported.body.data.user_tutorial, "viewer_id"), false)
    const shell = await requestInstance(personalService, {
        method: "POST",
        path: `/v1/local-saves/${slotId}/transfer-tokens/shell`,
        payload: { deviceName: "Three instance local L" },
        expectedStatus: 201,
    })
    const slot = await requestInstance(personalService, {
        method: "POST",
        path: `${localTransferPrefix}/shell/slot-tokens`,
        token: shell.body.token,
        payload: { slotId, permission: "both", deviceName: "Three instance local L slot" },
        expectedStatus: 201,
    })
    assert.match(slot.body.token, /^spt_slot_[A-Za-z0-9_-]{40,}$/)
    return {
        instanceId: shell.body.instanceId,
        package: exported.body,
        slotId,
        transfer: {
            baseUrl: personalService.baseUrl,
            prefix: localTransferPrefix,
            slotId,
            token: slot.body.token,
        },
        viewerId: signup.data_headers.viewer_id,
    }
}

async function prepareNodeBInstance(nodeB) {
    const signup = await signupCn(nodeB.baseUrl, 93002, "three-instance-node-b")
    const saves = await requestInstance(nodeB, {
        method: "GET",
        path: "/manage/api/saves",
        expectedStatus: 200,
    })
    assert.equal(saves.body.players.length, 1)
    return {
        targetPlayerId: saves.body.players[0].id,
        viewerId: signup.data_headers.viewer_id,
    }
}

async function importAndActivateNodeSlot(instance, portablePackage) {
    const imported = await importNodeSlot(instance, portablePackage)
    const playerId = imported.playerId
    const activated = await requestInstance(instance, {
        method: "POST",
        path: `/manage/api/saves/${playerId}/activate`,
        expectedStatus: 200,
    })
    assert.equal(activated.body.playerId, playerId)
    return imported
}

async function importNodeSlot(instance, portablePackage) {
    const imported = await requestInstance(instance, {
        method: "POST",
        path: `/manage/api/saves/${instance.targetPlayerId}/slots`,
        payload: portablePackage,
        expectedStatus: 201,
    })
    const playerId = imported.body.playerId
    return { playerId }
}

async function issueNodeShellToken(instance, playerId, deviceName) {
    const response = await requestInstance(instance, {
        method: "POST",
        path: `/manage/api/transfer/shells/${playerId}/tokens`,
        payload: { deviceName },
        expectedStatus: 201,
    })
    assert.match(response.body.instanceId, /^[a-f0-9]{32}$/)
    return response.body
}

async function issueNodeTransferAccess(instance, playerId, deviceName) {
    const shell = await issueNodeShellToken(instance, playerId, deviceName)
    const slot = await requestInstance(instance, {
        method: "POST",
        path: `${nodeTransferPrefix}/shell/slot-tokens`,
        token: shell.token,
        payload: { playerId, permission: "both", deviceName: `${deviceName} slot` },
        expectedStatus: 201,
    })
    assert.match(slot.body.token, /^spt_slot_[A-Za-z0-9_-]{40,}$/)
    return {
        baseUrl: instance.baseUrl,
        prefix: nodeTransferPrefix,
        slotId: playerId,
        token: slot.body.token,
        instanceId: shell.instanceId,
        shellId: String(slot.body.metadata.accountId),
    }
}

async function issueLocalTransferAccess(instance, slotId, deviceName) {
    const shell = await requestInstance(instance, {
        method: "POST",
        path: `/v1/local-saves/${slotId}/transfer-tokens/shell`,
        payload: { deviceName },
        expectedStatus: 201,
    })
    const slot = await requestInstance(instance, {
        method: "POST",
        path: `${localTransferPrefix}/shell/slot-tokens`,
        token: shell.body.token,
        payload: { slotId, permission: "both", deviceName: `${deviceName} slot` },
        expectedStatus: 201,
    })
    return {
        baseUrl: instance.baseUrl,
        prefix: localTransferPrefix,
        slotId,
        token: slot.body.token,
        instanceId: shell.body.instanceId,
    }
}
// //// /准备实例身份和初始槽 ////

// //// 创建分支并拒绝过期 revision [@x380kkm 2026-08-03] ////
async function createIndependentNodeBranches(nodeA, nodeAInitial, nodeB, nodeBInitial) {
    const nodeAPackage = createBranchPackage(nodeAInitial.body, "Three instance branch A")
    const nodeBPackage = createBranchPackage(nodeBInitial.body, "Three instance branch B")
    const nodeABranch = await overwriteTransferSlot(nodeA, nodeAInitial.etag, nodeAPackage)
    const nodeBBranch = await overwriteTransferSlot(nodeB, nodeBInitial.etag, nodeBPackage)
    const nodeAExport = await exportTransferSlot(nodeA)
    const nodeBExport = await exportTransferSlot(nodeB)
    assert.equal(nodeAExport.body.data.user_info.name, "Three instance branch A")
    assert.equal(nodeBExport.body.data.user_info.name, "Three instance branch B")
    assert.notEqual(nodeAExport.body.payloadSha256, nodeBExport.body.payloadSha256)
    return {
        nodeA: { response: nodeABranch, export: nodeAExport },
        nodeB: { response: nodeBBranch, export: nodeBExport },
    }
}

async function verifyStaleNodeUploadIsRejected(instance, transfer, initial, branch, sourcePackage) {
    const rejectedPackage = createBranchPackage(sourcePackage, "Rejected stale branch")
    const conflict = await overwriteTransferSlot(transfer, initial.etag, rejectedPackage, 409)
    assert.equal(conflict.body.error, "save_revision_conflict")
    assert.equal(conflict.body.currentEtag, branch.response.body.etag)
    const revisions = await requestInstance(instance, {
        method: "GET",
        path: `/manage/api/saves/${transfer.slotId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(revisions.body.currentRevisionId, branch.response.body.revisionId)
    assert.equal(
        revisions.body.revisions.some((revision) => revision.id === initial.body.source.revisionId),
        true,
    )
    const current = await exportTransferSlot(transfer)
    assert.equal(current.body.data.user_info.name, "Three instance branch A")
}

// //// 验证服务器间搬运和 revision 恢复 [@x380kkm 2026-08-04] ////
async function verifyNodeToNodeTransferAndRestore(
    nodeATransfer,
    nodeB,
    nodeBTransfer,
    branches,
) {
    const transferred = await overwriteTransferSlot(
        nodeBTransfer,
        branches.nodeB.export.etag,
        branches.nodeA.export.body,
    )
    assert.equal(transferred.body.imported, true)
    const nodeBCurrent = await exportTransferSlot(nodeBTransfer)
    assert.equal(nodeBCurrent.body.data.user_info.name, "Three instance branch A")
    assert.equal(nodeBCurrent.body.payloadSha256, branches.nodeA.export.body.payloadSha256)
    const nodeACurrent = await exportTransferSlot(nodeATransfer)
    assert.equal(nodeACurrent.body.payloadSha256, branches.nodeA.export.body.payloadSha256)

    const revisions = await requestInstance(nodeB, {
        method: "GET",
        path: `/manage/api/saves/${nodeBTransfer.slotId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(revisions.body.currentRevisionId, transferred.body.revisionId)
    assert.equal(
        revisions.body.revisions.some(
            (revision) => revision.id === branches.nodeB.response.body.revisionId,
        ),
        true,
    )
    const restored = await requestInstance(nodeB, {
        method: "POST",
        path: `/manage/api/saves/${nodeBTransfer.slotId}/revisions/${branches.nodeB.response.body.revisionId}/restore`,
        headers: { "if-match": transferred.body.etag },
        expectedStatus: 200,
    })
    assert.equal(restored.body.restored, true)
    const nodeBAfterRestore = await exportTransferSlot(nodeBTransfer)
    assert.equal(nodeBAfterRestore.body.data.user_info.name, "Three instance branch B")
    assert.equal(nodeBAfterRestore.body.payloadSha256, branches.nodeB.export.body.payloadSha256)
    const revisionsAfterRestore = await requestInstance(nodeB, {
        method: "GET",
        path: `/manage/api/saves/${nodeBTransfer.slotId}/revisions`,
        expectedStatus: 200,
    })
    assert.equal(revisionsAfterRestore.body.currentRevisionId, restored.body.revision.id)
    assert.equal(
        revisionsAfterRestore.body.revisions.some(
            (revision) => revision.id === transferred.body.revisionId,
        ),
        true,
    )
    const nodeAAfterRestore = await exportTransferSlot(nodeATransfer)
    assert.equal(nodeAAfterRestore.body.data.user_info.name, "Three instance branch A")
    assert.equal(nodeAAfterRestore.etag, branches.nodeA.export.etag)
    assert.equal(nodeAAfterRestore.body.payloadSha256, branches.nodeA.export.body.payloadSha256)
}
// //// /验证服务器间搬运和 revision 恢复 ////

async function verifySlotTokenCannotAccessAnotherSlot(transfer, otherSlotId) {
    const rejected = await requestJson(transfer.baseUrl, {
        method: "GET",
        path: `${transfer.prefix}/slots/${otherSlotId}`,
        token: transfer.token,
        expectedStatus: 401,
    })
    assert.equal(rejected.body.error, "transfer_token_required")
}

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

function overwriteTransferSlot(transfer, etag, portablePackage, expectedStatus = 200) {
    return requestJson(transfer.baseUrl, {
        method: "PUT",
        path: `${transfer.prefix}/slots/${transfer.slotId}`,
        headers: { "if-match": etag },
        token: transfer.token,
        payload: portablePackage,
        expectedStatus,
    })
}

function exportTransferSlot(transfer) {
    return requestJson(transfer.baseUrl, {
        method: "GET",
        path: `${transfer.prefix}/slots/${transfer.slotId}`,
        token: transfer.token,
        expectedStatus: 200,
    })
}
// //// /创建分支并拒绝过期 revision ////

// //// 发送实例管理请求 [@x380kkm 2026-08-03] ////
function exportNodeSlot(instance, playerId) {
    return requestInstance(instance, {
        method: "GET",
        path: `/manage/api/saves/${playerId}`,
        expectedStatus: 200,
    })
}

function requestInstance(instance, request) {
    const token = Object.hasOwn(request, "token") ? request.token : instance.managementToken
    return requestJson(instance.baseUrl, {
        ...request,
        token,
    })
}
// //// /发送实例管理请求 ////

module.exports = { verifyThreeInstancePortableSave }
