// audience: internal
// # management-api-test
// 此脚本在临时目录中注入编译后的管理插件并验证认证, 权限, 账号隔离和多槽存档管理.
// 指定 personal-service-save-sync 参数时, 测试临时监听 loopback 并验证三个服务实例.

const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const Fastify = require("fastify")
const Database = require("better-sqlite3")
const { verifyEncryptedSaveStorage } = require("./management-encrypted-save-test")
const { verifyPersonalServiceSaveSync } = require("./management-personal-save-sync-test")
const { verifyThreeInstancePortableSave } = require("./management-three-instance-portable-save-test")

const repositoryRoot = path.resolve(__dirname, "..", "..")
const managementPluginPath = path.join(repositoryRoot, "out", "routes", "management.js")
const dataModulePath = path.join(repositoryRoot, "out", "data", "wdfpData.js")
const dataTypesPath = path.join(repositoryRoot, "out", "data", "types.js")
const databaseModulePath = path.join(repositoryRoot, "out", "data", "index.js")
const databaseInitializerModulePath = path.join(repositoryRoot, "out", "data", "initializers", "wdfpData.js")
const managementStoreModulePath = path.join(repositoryRoot, "out", "control", "management.js")
const managementAccessModulePath = path.join(repositoryRoot, "out", "control", "managementAccess.js")
const battleSettlementStateModulePath = path.join(repositoryRoot, "out", "control", "battleSettlementState.js")
const matchmakingStoreModulePath = path.join(repositoryRoot, "out", "multiplayer", "matchmakingStore.js")
const playerMailsModulePath = path.join(repositoryRoot, "out", "data", "playerMails.js")
const adminToken = "management-test-token"
const playerCredentials = Object.freeze({
    username: "test-player",
    password: "player-password-123",
})

// //// 创建账号和会话隔离测试数据 [@x380kkm 2026-07-22] ////
async function createAccountFixture(data, SessionType, suffix) {
    const account = await data.insertAccount({
        appId: "starpoint-cn",
        idpAlias: `Account ${suffix}`,
        idpCode: "leiting",
        idpId: `private-idp-${suffix}`,
        status: "active",
    })
    await data.insertSessionWithToken({
        token: `private-zat-${suffix}`,
        expires: new Date(Date.now() + 60_000),
        type: SessionType.ZAT,
        accountId: account.id,
    })
    const viewerSession = await data.generateViewerIdSession(account.id)
    const player = data.insertDefaultPlayerSync(account.id)
    return { account, viewerSession, player }
}
// //// /创建账号和会话隔离测试数据 ////

// //// 验证统一 bearer token 认证和安全响应头 [@x380kkm 2026-07-22] ////
async function verifyAuthentication(app) {
    const page = await app.inject({ method: "GET", url: "/manage/" })
    assert.equal(page.statusCode, 200)
    assert.equal(page.headers["cache-control"], "no-store")
    assert.match(page.headers["content-security-policy"], /frame-ancestors 'none'/)
    assert.match(page.body, /id="send-mail"/)

    delete process.env.MANAGEMENT_ADMIN_TOKEN
    const unconfigured = await app.inject({ method: "GET", url: "/manage/api/accounts" })
    assert.equal(unconfigured.statusCode, 503)

    process.env.MANAGEMENT_ADMIN_TOKEN = adminToken
    const unauthorized = await app.inject({
        method: "GET",
        url: "/manage/api/accounts",
        headers: { authorization: "Bearer wrong-token" },
    })
    assert.equal(unauthorized.statusCode, 401)
    assert.equal(unauthorized.headers["www-authenticate"], "Bearer")
    assert.equal(unauthorized.headers["cache-control"], "no-store")
}
// //// /验证统一 bearer token 认证和安全响应头 ////

// //// 验证账号分页脱敏和单账号会话控制 [@x380kkm 2026-07-22] ////
async function verifyAccountAdministration(app, data, SessionType) {
    const first = await createAccountFixture(data, SessionType, "first")
    const second = await createAccountFixture(data, SessionType, "second")
    const headers = { authorization: `Bearer ${adminToken}` }

    const firstPageResponse = await app.inject({
        method: "GET",
        url: "/manage/api/accounts?limit=1&offset=0",
        headers,
    })
    assert.equal(firstPageResponse.statusCode, 200)
    const firstPage = firstPageResponse.json()
    assert.deepEqual([firstPage.total, firstPage.limit, firstPage.offset], [2, 1, 0])
    assert.equal(firstPage.accounts[0].id, first.account.id)
    assert.equal(firstPage.accounts[0].activePlayerId, first.player.id)
    assert.deepEqual(firstPage.accounts[0].sessionCounts, { zat: 1, zrt: 0, viewer: 1 })
    const serializedPage = JSON.stringify(firstPage)
    assert.doesNotMatch(serializedPage, /private-idp|private-zat/)
    assert.equal(Object.hasOwn(firstPage.accounts[0], "idpId"), false)
    assert.equal(Object.hasOwn(firstPage.accounts[0], "idpAlias"), false)

    const secondPageResponse = await app.inject({
        method: "GET",
        url: "/manage/api/accounts?limit=1&offset=1",
        headers,
    })
    assert.equal(secondPageResponse.json().accounts[0].id, second.account.id)

    const revokeResponse = await app.inject({
        method: "DELETE",
        url: `/manage/api/accounts/${first.account.id}/sessions`,
        headers,
    })
    assert.equal(revokeResponse.statusCode, 200)
    assert.equal(revokeResponse.json().revokedSessions, 2)
    assert.equal(await data.getSession(first.viewerSession.token), null)
    assert.notEqual(await data.getSession(second.viewerSession.token), null)

    const rotateResponse = await app.inject({
        method: "POST",
        url: `/manage/api/accounts/${first.account.id}/viewer-id`,
        headers,
    })
    assert.equal(rotateResponse.statusCode, 200)
    const rotatedViewerId = rotateResponse.json().viewerId
    assert.equal(typeof rotatedViewerId, "string")
    assert.equal((await data.getSession(rotatedViewerId)).accountId, first.account.id)
    assert.equal((await data.getSession(second.viewerSession.token)).accountId, second.account.id)

    const invalidAccount = await app.inject({
        method: "DELETE",
        url: "/manage/api/accounts/not-a-number/sessions",
        headers,
    })
    assert.equal(invalidAccount.statusCode, 400)
    const missingAccount = await app.inject({
        method: "POST",
        url: "/manage/api/accounts/999999/viewer-id",
        headers,
    })
    assert.equal(missingAccount.statusCode, 404)
    return { first: { ...first, activeViewerId: rotatedViewerId }, second }
}
// //// /验证账号分页脱敏和单账号会话控制 ////

// //// 验证旧账号回填激活槽并拒绝跨账号切换 [@x380kkm 2026-07-27] ////
function verifyActiveSlotBackfill(database, initializeDatabase, data, fixtures) {
    const additionalPlayer = data.insertDefaultPlayerSync(fixtures.second.account.id)
    database.prepare("DELETE FROM account_active_players WHERE account_id = ?").run(fixtures.second.account.id)
    assert.equal(data.getActivePlayerIdSync(fixtures.second.account.id), null)

    initializeDatabase(database, true)
    assert.equal(data.getActivePlayerIdSync(fixtures.second.account.id), fixtures.second.player.id)
    assert.deepEqual(
        data.getAccountPlayersSync(fixtures.second.account.id),
        [fixtures.second.player.id, additionalPlayer.id],
    )
    assert.throws(
        () => data.activateAccountPlayerSync(fixtures.first.account.id, additionalPlayer.id),
        /does not own/,
    )
}
// //// /验证旧账号回填激活槽并拒绝跨账号切换 ////

// //// 验证 SQLite 登录, 两级权限和本人多槽存档管理 [@x380kkm 2026-07-27] ////
async function verifyUserAccess(app, accessStore, data, battleSettlementState, matchmakingStore, fixtures) {
    await accessStore.createUser("test-admin", "correct-horse-battery-staple", "admin")
    const invalidLogin = await app.inject({
        method: "POST",
        url: "/manage/api/auth/login",
        payload: { username: "test-admin", password: "incorrect-password" },
    })
    assert.equal(invalidLogin.statusCode, 401)

    const adminLogin = await app.inject({
        method: "POST",
        url: "/manage/api/auth/login",
        payload: { username: "test-admin", password: "correct-horse-battery-staple" },
    })
    assert.equal(adminLogin.statusCode, 200)
    const adminSetCookie = adminLogin.headers["set-cookie"]
    assert.equal(typeof adminSetCookie, "string")
    assert.match(adminSetCookie, /HttpOnly/)
    assert.match(adminSetCookie, /SameSite=Strict/)
    assert.match(adminSetCookie, /Path=\/manage/)
    const adminCookie = adminSetCookie.split(";", 1)[0]

    const session = await app.inject({ method: "GET", url: "/manage/api/auth/session", headers: { cookie: adminCookie } })
    assert.equal(session.json().user.role, "admin")
    assert.equal(session.headers["cache-control"], "no-store")
    const createUser = await app.inject({
        method: "POST",
        url: "/manage/api/users",
        headers: { cookie: adminCookie },
        payload: { ...playerCredentials, role: "player" },
    })
    assert.equal(createUser.statusCode, 200)
    const playerUser = createUser.json()
    assert.equal(Object.hasOwn(playerUser, "password_hash"), false)

    const binding = await app.inject({
        method: "PUT",
        url: `/manage/api/users/${playerUser.id}/players/${fixtures.first.player.id}`,
        headers: { cookie: adminCookie },
    })
    assert.equal(binding.statusCode, 200)

    const playerLogin = await app.inject({
        method: "POST",
        url: "/manage/api/auth/login",
        payload: playerCredentials,
    })
    assert.equal(playerLogin.statusCode, 200)
    const playerCookie = playerLogin.headers["set-cookie"].split(";", 1)[0]
    const forbiddenStatus = await app.inject({ method: "GET", url: "/manage/api/status", headers: { cookie: playerCookie } })
    assert.equal(forbiddenStatus.statusCode, 403)
    const forbiddenUsers = await app.inject({ method: "GET", url: "/manage/api/users", headers: { cookie: playerCookie } })
    assert.equal(forbiddenUsers.statusCode, 403)
    const forbiddenTransferBindings = await app.inject({
        method: "GET",
        url: `/manage/api/saves/${fixtures.first.player.id}/transfer-bindings`,
        headers: { cookie: playerCookie },
    })
    assert.equal(forbiddenTransferBindings.statusCode, 403)

    const saves = await app.inject({ method: "GET", url: "/manage/api/saves", headers: { cookie: playerCookie } })
    assert.deepEqual(saves.json().players.map((player) => player.id), [fixtures.first.player.id])
    assert.equal(saves.json().players[0].active, true)
    assert.equal(saves.json().players[0].accountId, fixtures.first.account.id)
    const exported = await app.inject({
        method: "GET",
        url: `/manage/api/saves/${fixtures.first.player.id}`,
        headers: { cookie: playerCookie },
    })
    assert.equal(exported.statusCode, 200)
    assert.match(exported.headers["content-disposition"], /attachment/)
    assert.equal(exported.json().format, "starpoint-save-package")
    assert.equal(exported.json().version, 1)
    assert.match(exported.json().payloadSha256, /^[a-f0-9]{64}$/)
    assert.equal(Object.hasOwn(exported.json().data.user_tutorial, "viewer_id"), false)
    assert.equal(Object.hasOwn(exported.json().data, "associate_token"), false)
    assert.match(exported.json().source.revisionId, /^[0-9a-f-]{36}$/)
    assert.match(exported.headers.etag, /^"[a-f0-9]{64}"$/)

    const importedSlotResponse = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${fixtures.first.player.id}/slots`,
        headers: { cookie: playerCookie },
        payload: exported.json(),
    })
    assert.equal(importedSlotResponse.statusCode, 201)
    const importedPlayerId = importedSlotResponse.json().playerId
    assert.notEqual(importedPlayerId, fixtures.first.player.id)
    assert.equal(importedSlotResponse.json().active, false)
    assert.deepEqual(accessStore.getBoundPlayerIds(playerUser.id), [fixtures.first.player.id])
    assert.deepEqual(await data.getAccountPlayers(fixtures.first.account.id), [fixtures.first.player.id, importedPlayerId])

    const savesWithImportedSlot = await app.inject({ method: "GET", url: "/manage/api/saves", headers: { cookie: playerCookie } })
    assert.deepEqual(
        savesWithImportedSlot.json().players.map((player) => player.id),
        [fixtures.first.player.id, importedPlayerId],
    )
    const adminSaves = await app.inject({ method: "GET", url: "/manage/api/saves", headers: { cookie: adminCookie } })
    assert.deepEqual(
        adminSaves.json().players.map((player) => player.id),
        data.getAllPlayerIdsSync(),
    )

    const activateImportedSlot = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${importedPlayerId}/activate`,
        headers: { cookie: playerCookie },
    })
    assert.equal(activateImportedSlot.statusCode, 200)
    assert.equal(data.getActivePlayerIdSync(fixtures.first.account.id), importedPlayerId)
    assert.deepEqual(await data.getAccountPlayers(fixtures.first.account.id), [importedPlayerId, fixtures.first.player.id])
    assert.equal(data.getPlayerFromAccountIdSync(fixtures.first.account.id).id, importedPlayerId)
    assert.equal((await data.getSession(fixtures.first.activeViewerId)).accountId, fixtures.first.account.id)

    battleSettlementState.insertActiveQuest(importedPlayerId, {
        questId: 1,
        playId: "save-slot-switch-test",
        category: 1,
        useBossBoostPoint: false,
        useBoostPoint: false,
        isAutoStartMode: false,
    })
    const battleBlockedActivation = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${fixtures.first.player.id}/activate`,
        headers: { cookie: playerCookie },
    })
    assert.equal(battleBlockedActivation.statusCode, 409)
    assert.equal(battleBlockedActivation.json().blockedBy, "battle_or_settlement")
    battleSettlementState.deleteActiveQuest(importedPlayerId)

    const room = matchmakingStore.createRoom({
        hostAccountId: fixtures.first.account.id,
        hostViewerId: Number(fixtures.first.activeViewerId),
        categoryId: 1,
        questId: 1,
        partyId: 1,
    })
    const roomBlockedActivation = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${fixtures.first.player.id}/activate`,
        headers: { cookie: playerCookie },
    })
    assert.equal(roomBlockedActivation.statusCode, 409)
    assert.equal(roomBlockedActivation.json().blockedBy, "room")

    matchmakingStore.setParticipantConnection(
        room.roomNumber,
        Number(fixtures.first.activeViewerId),
        "save-slot-test-connection",
    )
    const battleStart = matchmakingStore.startBattle(room.roomNumber, {
        accountId: fixtures.first.account.id,
        viewerId: Number(fixtures.first.activeViewerId),
    }, "save-slot-room-battle")
    assert.notEqual(battleStart, null)
    battleSettlementState.insertActiveQuest(importedPlayerId, {
        questId: 1,
        playId: "save-slot-room-battle",
        category: 1,
        useBossBoostPoint: false,
        useBoostPoint: false,
        isAutoStartMode: false,
    })
    const settlementBlockedActivation = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${fixtures.first.player.id}/activate`,
        headers: { cookie: playerCookie },
    })
    assert.equal(settlementBlockedActivation.statusCode, 409)
    assert.equal(settlementBlockedActivation.json().blockedBy, "battle_or_settlement")
    battleSettlementState.deleteActiveQuest(importedPlayerId)

    const activateOriginalSlot = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${fixtures.first.player.id}/activate`,
        headers: { cookie: playerCookie },
    })
    assert.equal(activateOriginalSlot.statusCode, 200)
    assert.equal(data.getActivePlayerIdSync(fixtures.first.account.id), fixtures.first.player.id)
    assert.equal(matchmakingStore.disbandRoom(room.roomNumber, fixtures.first.account.id), true)

    const forbiddenSave = await app.inject({
        method: "GET",
        url: `/manage/api/saves/${fixtures.second.player.id}`,
        headers: { cookie: playerCookie },
    })
    assert.equal(forbiddenSave.statusCode, 403)
    const tamperedSave = structuredClone(exported.json())
    tamperedSave.data.user_info.name = "Tampered without updating the digest"
    const rejectedTamperedSave = await app.inject({
        method: "PUT",
        url: `/manage/api/saves/${fixtures.first.player.id}`,
        headers: { cookie: playerCookie },
        payload: tamperedSave,
    })
    assert.equal(rejectedTamperedSave.statusCode, 400)
    assert.equal(rejectedTamperedSave.json().error, "invalid_save_package")

    // //// 验证覆盖前 revision 和并发冲突保留分支 [@x380kkm 2026-07-27] ////
    const initialRevisionId = exported.json().source.revisionId
    const firstBranchData = structuredClone(exported.json().data)
    firstBranchData.user_info.name = "Revision Branch"
    const firstBranch = await app.inject({
        method: "PUT",
        url: `/manage/api/saves/${fixtures.first.player.id}`,
        headers: { cookie: playerCookie, "if-match": exported.headers.etag },
        payload: firstBranchData,
    })
    assert.equal(firstBranch.statusCode, 200)
    assert.notEqual(firstBranch.json().revision.id, initialRevisionId)
    assert.equal(firstBranch.json().revision.parentRevisionId, initialRevisionId)
    const firstBranchRevisionId = firstBranch.json().revision.id

    const staleOverwrite = await app.inject({
        method: "PUT",
        url: `/manage/api/saves/${fixtures.first.player.id}`,
        headers: { cookie: playerCookie, "if-match": exported.headers.etag },
        payload: exported.json(),
    })
    assert.equal(staleOverwrite.statusCode, 409)
    assert.equal(staleOverwrite.json().error, "save_revision_conflict")
    assert.equal(staleOverwrite.json().currentRevisionId, firstBranchRevisionId)
    assert.equal(data.getPlayerSync(fixtures.first.player.id).name, "Revision Branch")

    const revisionList = await app.inject({
        method: "GET",
        url: `/manage/api/saves/${fixtures.first.player.id}/revisions`,
        headers: { cookie: playerCookie },
    })
    assert.equal(revisionList.statusCode, 200)
    assert.equal(revisionList.json().currentRevisionId, firstBranchRevisionId)
    assert.deepEqual(
        new Set(revisionList.json().revisions.map((revision) => revision.id)),
        new Set([firstBranchRevisionId, initialRevisionId]),
    )

    const restoredInitial = await app.inject({
        method: "POST",
        url: `/manage/api/saves/${fixtures.first.player.id}/revisions/${initialRevisionId}/restore`,
        headers: { cookie: playerCookie, "if-match": firstBranch.headers.etag },
    })
    assert.equal(restoredInitial.statusCode, 200)
    assert.equal(restoredInitial.json().revision.parentRevisionId, firstBranchRevisionId)
    assert.equal(data.getPlayerSync(fixtures.first.player.id).name, fixtures.first.player.name)
    // //// /验证覆盖前 revision 和并发冲突保留分支 ////

    const imported = await app.inject({
        method: "PUT",
        url: `/manage/api/saves/${fixtures.first.player.id}`,
        headers: { cookie: playerCookie },
        payload: exported.json(),
    })
    assert.equal(imported.statusCode, 200)
    assert.equal(imported.json().imported, true)
    assert.equal(data.getActivePlayerIdSync(fixtures.first.account.id), fixtures.first.player.id)

    // //// 验证壳 token 和槽 token 的最小权限边界 [@x380kkm 2026-07-27] ////
    const shellTokenResponse = await app.inject({
        method: "POST",
        url: `/manage/api/transfer/shells/${fixtures.first.player.id}/tokens`,
        headers: { cookie: playerCookie },
        payload: {
            deviceName: "Test transfer device",
            expiresAt: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
        },
    })
    assert.equal(shellTokenResponse.statusCode, 201)
    const shellToken = shellTokenResponse.json().token
    assert.match(shellToken, /^spt_shell_[A-Za-z0-9_-]{40,}$/)
    assert.match(shellTokenResponse.json().instanceId, /^[a-f0-9]{32}$/)
    assert.equal(shellTokenResponse.json().metadata.accountId, fixtures.first.account.id)

    const shellSlots = await app.inject({
        method: "GET",
        url: "/manage/transfer/v1/shell/slots",
        headers: { authorization: `Bearer ${shellToken}` },
    })
    assert.equal(shellSlots.statusCode, 200)
    assert.deepEqual(
        new Set(shellSlots.json().slots.map((slot) => slot.id)),
        new Set([fixtures.first.player.id, importedPlayerId]),
    )

    const slotTokenResponse = await app.inject({
        method: "POST",
        url: "/manage/transfer/v1/shell/slot-tokens",
        headers: { authorization: `Bearer ${shellToken}` },
        payload: { playerId: fixtures.first.player.id, permission: "download", deviceName: "Download device" },
    })
    assert.equal(slotTokenResponse.statusCode, 201)
    const downloadToken = slotTokenResponse.json().token
    assert.match(downloadToken, /^spt_slot_[A-Za-z0-9_-]{40,}$/)
    assert.equal(slotTokenResponse.json().metadata.playerId, fixtures.first.player.id)
    assert.equal(slotTokenResponse.json().metadata.permission, "download")

    const tokenList = await app.inject({
        method: "GET",
        url: `/manage/api/transfer/slots/${fixtures.first.player.id}/tokens`,
        headers: { cookie: playerCookie },
    })
    assert.equal(tokenList.statusCode, 200)
    assert.equal(JSON.stringify(tokenList.json()).includes(downloadToken), false)
    assert.equal(tokenList.json().tokens.some((token) => token.id === slotTokenResponse.json().metadata.id), true)

    const downloaded = await app.inject({
        method: "GET",
        url: `/manage/transfer/v1/slots/${fixtures.first.player.id}`,
        headers: { authorization: `Bearer ${downloadToken}` },
    })
    assert.equal(downloaded.statusCode, 200)
    assert.equal(downloaded.json().format, "starpoint-save-package")
    assert.match(downloaded.headers.etag, /^"[a-f0-9]{64}"$/)

    const wrongSlot = await app.inject({
        method: "GET",
        url: `/manage/transfer/v1/slots/${importedPlayerId}`,
        headers: { authorization: `Bearer ${downloadToken}` },
    })
    assert.equal(wrongSlot.statusCode, 401)

    const wrongPermission = await app.inject({
        method: "PUT",
        url: `/manage/transfer/v1/slots/${fixtures.first.player.id}`,
        headers: { authorization: `Bearer ${downloadToken}` },
        payload: downloaded.json(),
    })
    assert.equal(wrongPermission.statusCode, 401)

    const uploadTokenResponse = await app.inject({
        method: "POST",
        url: "/manage/transfer/v1/shell/slot-tokens",
        headers: { authorization: `Bearer ${shellToken}` },
        payload: { playerId: fixtures.first.player.id, permission: "upload" },
    })
    assert.equal(uploadTokenResponse.statusCode, 201)
    battleSettlementState.insertActiveQuest(fixtures.first.player.id, {
        questId: 1,
        playId: "transfer-token-upload-block",
        category: 1,
        useBossBoostPoint: false,
        useBoostPoint: false,
        isAutoStartMode: false,
    })
    const blockedUpload = await app.inject({
        method: "PUT",
        url: `/manage/transfer/v1/slots/${fixtures.first.player.id}`,
        headers: { authorization: `Bearer ${uploadTokenResponse.json().token}` },
        payload: downloaded.json(),
    })
    assert.equal(blockedUpload.statusCode, 409)
    assert.equal(blockedUpload.json().error, "save_slot_import_blocked")
    battleSettlementState.deleteActiveQuest(fixtures.first.player.id)
    const uploaded = await app.inject({
        method: "PUT",
        url: `/manage/transfer/v1/slots/${fixtures.first.player.id}`,
        headers: {
            authorization: `Bearer ${uploadTokenResponse.json().token}`,
            "if-match": downloaded.headers.etag,
        },
        payload: downloaded.json(),
    })
    assert.equal(uploaded.statusCode, 200)
    assert.equal(uploaded.json().imported, true)

    const revoked = await app.inject({
        method: "DELETE",
        url: `/manage/transfer/v1/shell/slots/${fixtures.first.player.id}/tokens/${slotTokenResponse.json().metadata.id}`,
        headers: { authorization: `Bearer ${shellToken}` },
    })
    assert.equal(revoked.statusCode, 200)
    const revokedDownload = await app.inject({
        method: "GET",
        url: `/manage/transfer/v1/slots/${fixtures.first.player.id}`,
        headers: { authorization: `Bearer ${downloadToken}` },
    })
    assert.equal(revokedDownload.statusCode, 401)

    const expiredTokenResponse = await app.inject({
        method: "POST",
        url: "/manage/transfer/v1/shell/slot-tokens",
        headers: { authorization: `Bearer ${shellToken}` },
        payload: { playerId: fixtures.first.player.id, permission: "download" },
    })
    assert.equal(expiredTokenResponse.statusCode, 201)

    const forbiddenShell = await app.inject({
        method: "POST",
        url: `/manage/api/transfer/shells/${fixtures.second.player.id}/tokens`,
        headers: { cookie: playerCookie },
        payload: {},
    })
    assert.equal(forbiddenShell.statusCode, 403)

    const tokenDatabase = new Database(accessStore.databasePath, { readonly: true })
    const persistedSlotToken = tokenDatabase.prepare(`
        SELECT token_hash
        FROM transfer_slot_tokens
        WHERE id = ?
    `).get(slotTokenResponse.json().metadata.id)
    tokenDatabase.close()
    assert.notEqual(persistedSlotToken.token_hash, downloadToken)
    assert.equal(persistedSlotToken.token_hash.includes(downloadToken), false)

    const writableTokenDatabase = new Database(accessStore.databasePath)
    writableTokenDatabase.prepare(`
        UPDATE transfer_slot_tokens
        SET expires_at = ?
        WHERE id = ?
    `).run(Math.floor(Date.now() / 1000) - 1, expiredTokenResponse.json().metadata.id)
    writableTokenDatabase.close()
    const expiredDownload = await app.inject({
        method: "GET",
        url: `/manage/transfer/v1/slots/${fixtures.first.player.id}`,
        headers: { authorization: `Bearer ${expiredTokenResponse.json().token}` },
    })
    assert.equal(expiredDownload.statusCode, 401)
    // //// /验证壳 token 和槽 token 的最小权限边界 ////

    const rawToken = decodeURIComponent(playerCookie.split("=", 2)[1])
    const accessDatabase = new Database(accessStore.databasePath, { readonly: true })
    const persistedSession = accessDatabase.prepare("SELECT token_hash FROM management_sessions ORDER BY created_at DESC LIMIT 1").get()
    accessDatabase.close()
    assert.notEqual(persistedSession.token_hash, rawToken)

    await verifyEncryptedSaveStorage(app, accessStore, adminCookie, playerCookie, adminToken)

    await app.inject({ method: "POST", url: "/manage/api/auth/logout", headers: { cookie: playerCookie } })
    const loggedOut = await app.inject({ method: "GET", url: "/manage/api/saves", headers: { cookie: playerCookie } })
    assert.equal(loggedOut.statusCode, 401)
}
// //// /验证 SQLite 登录, 两级权限和本人多槽存档管理 ////

// //// 验证管理员发放邮件和一次性领取 [@x380kkm 2026-07-24] ////
async function verifyMailAdministration(app, fixtures) {
    const headers = { authorization: `Bearer ${adminToken}` }
    const created = await app.inject({
        method: "POST",
        url: "/manage/api/mails",
        headers,
        payload: {
            playerId: fixtures.first.player.id,
            title: "测试补给",
            body: "来自管理服务的测试邮件",
            sender: "Starpoint",
            rewards: { itemList: { "100000": 7 }, freeVmoney: 120, freeMana: 45 },
        },
    })
    assert.equal(created.statusCode, 200)
    const mail = created.json()
    assert.equal(mail.playerId, fixtures.first.player.id)
    assert.equal(mail.rewards.itemList["100000"], 7)

    const listed = await app.inject({
        method: "GET",
        url: `/manage/api/mails/${fixtures.first.player.id}`,
        headers,
    })
    assert.equal(listed.statusCode, 200)
    assert.equal(listed.json().total, 1)
    assert.equal(listed.json().mails[0].id, mail.id)

    const { claimPlayerMailSync } = require(playerMailsModulePath)
    const claim = claimPlayerMailSync(fixtures.first.player.id, mail.id)
    assert.deepEqual(claim.itemList, { "100000": 7 })
    assert.equal(claim.remainingCount, 0)
    const data = require(dataModulePath)
    const player = data.getPlayerSync(fixtures.first.player.id)
    assert.equal(player.freeVmoney, 270)
    assert.equal(player.freeMana, 1045)

    const emptyList = await app.inject({
        method: "GET",
        url: `/manage/api/mails/${fixtures.first.player.id}`,
        headers,
    })
    assert.equal(emptyList.json().total, 0)
}
// //// /验证管理员发放邮件和一次性领取 ////

// //// 验证虚拟时间在服务重启后继续按倍率推进 [@x380kkm 2026-07-22] ////
async function verifyVirtualTimePersistence(app, managementStore, ManagementStore) {
    const headers = { authorization: `Bearer ${adminToken}` }
    const baseIso = "2030-01-01T00:00:00.000Z"
    const setResponse = await app.inject({
        method: "PUT",
        url: "/manage/api/time",
        headers,
        payload: { enabled: true, iso: baseIso, rate: 4 },
    })
    assert.equal(setResponse.statusCode, 200)
    assert.equal(typeof setResponse.json().virtualTime.realTimeAnchor, "string")

    const config = await managementStore.load()
    const realTimeAnchor = new Date(Date.now() - 2_000)
    config.virtualTime.realTimeAnchor = realTimeAnchor.toISOString()
    await managementStore.save(config)
    const restartedStore = new ManagementStore({
        rootDir: managementStore.rootDir,
        statePath: managementStore.statePath,
        databasePath: managementStore.databasePath,
        backupDir: managementStore.backupDir,
    })
    await restartedStore.applyVirtualTime()

    const statusResponse = await app.inject({ method: "GET", url: "/manage/api/status", headers })
    const expected = Date.parse(baseIso) + (Date.now() - realTimeAnchor.getTime()) * 4
    assert.ok(Math.abs(Date.parse(statusResponse.json().serverDate) - expected) < 1_000)

    const resetResponse = await app.inject({
        method: "PUT",
        url: "/manage/api/time",
        headers,
        payload: { enabled: false },
    })
    assert.equal(resetResponse.statusCode, 200)
    assert.equal(resetResponse.json().virtualTime.realTimeAnchor, null)
}
// //// /验证虚拟时间在服务重启后继续按倍率推进 ////

// //// 验证 CN 新实例内容基线和手动时间设置持久化 [@x380kkm 2026-07-23] ////
async function verifyDefaultCnContentTime(managementStore, ManagementStore, baselineIso) {
    const initial = await managementStore.load()
    assert.equal(initial.instance.mode, "cn")
    assert.equal(initial.virtualTime.enabled, true)
    assert.equal(initial.virtualTime.iso, baselineIso)
    assert.equal(initial.virtualTime.rate, 1)
    assert.equal(typeof initial.virtualTime.realTimeAnchor, "string")
    assert.equal(fs.existsSync(managementStore.statePath), true)

    const disabled = await managementStore.setVirtualTime(false, null, 1)
    assert.equal(disabled.virtualTime.enabled, false)
    assert.equal(disabled.virtualTime.iso, null)

    const restartedStore = new ManagementStore({
        rootDir: managementStore.rootDir,
        statePath: managementStore.statePath,
        databasePath: managementStore.databasePath,
        backupDir: managementStore.backupDir,
    })
    const preserved = await restartedStore.load()
    assert.equal(preserved.virtualTime.enabled, false)
    assert.equal(preserved.virtualTime.iso, null)
}
// //// /验证 CN 新实例内容基线和手动时间设置持久化 ////

// //// 在运行中的 WAL 数据库上创建备份并暂存恢复 [@x380kkm 2026-07-22] ////
async function prepareBackupRestoreFixture(data, managementStore) {
    const manifest = await managementStore.createBackup()
    assert.equal(manifest.schemaVersion, 2)
    assert.equal(manifest.files.length, 1)
    assert.equal(manifest.files[0].name, path.basename(managementStore.databasePath))

    await data.insertAccount({
        appId: "starpoint-cn",
        idpAlias: "Account after backup",
        idpCode: "leiting",
        idpId: "private-idp-after-backup",
        status: "active",
    })
    const pending = await managementStore.stageRestore(manifest.id)
    assert.equal(pending.schemaVersion, 1)
    assert.equal(pending.backupId, manifest.id)
    assert.equal(pending.databaseExisted, true)
    return { manifest, pending }
}
// //// /在运行中的 WAL 数据库上创建备份并暂存恢复 ////

// //// 验证损坏暂存文件不改动原数据库且中断后可完成恢复 [@x380kkm 2026-07-22] ////
function countAccounts(databasePath) {
    const database = new Database(databasePath, { readonly: true, fileMustExist: true })
    try {
        return database.prepare("SELECT COUNT(*) AS total FROM accounts").get().total
    } finally {
        database.close()
    }
}

async function verifyBackupRestore(managementStore, fixture) {
    const stagedDatabasePath = `${managementStore.databasePath}.restore`
    fs.appendFileSync(stagedDatabasePath, "corrupt")
    await assert.rejects(managementStore.applyPendingRestore(), /checksum/)
    assert.equal(countAccounts(managementStore.databasePath), 3)

    await managementStore.stageRestore(fixture.manifest.id)
    const applied = await managementStore.applyPendingRestore()
    assert.equal(applied.backupId, fixture.manifest.id)
    assert.equal(applied.rollbackRetained, false)
    assert.equal(countAccounts(managementStore.databasePath), 2)
    assert.equal(await managementStore.getPendingRestore(), null)
    assert.equal(fs.existsSync(`${managementStore.databasePath}.rollback`), false)

    const interruptedManifest = await managementStore.createBackup()
    await managementStore.stageRestore(interruptedManifest.id)
    fs.renameSync(managementStore.databasePath, `${managementStore.databasePath}.rollback`)
    fs.renameSync(`${managementStore.databasePath}.restore`, managementStore.databasePath)
    const completed = await managementStore.applyPendingRestore()
    assert.equal(completed.backupId, interruptedManifest.id)
    assert.equal(countAccounts(managementStore.databasePath), 2)
    assert.equal(fs.existsSync(`${managementStore.databasePath}.rollback`), false)
}
// //// /验证损坏暂存文件不改动原数据库且中断后可完成恢复 ////

// //// 在临时数据库上装配并关闭管理插件 [@x380kkm 2026-07-22] ////
async function run() {
    assert.ok(fs.existsSync(managementPluginPath), "Run npm run build before the management API test.")
    const originalCwd = process.cwd()
    const originalToken = process.env.MANAGEMENT_ADMIN_TOKEN
    const originalStateFile = process.env.MANAGEMENT_STATE_FILE
    const originalDatabasePath = process.env.DATABASE_PATH
    const originalAccessDatabasePath = process.env.MANAGEMENT_ACCESS_DATABASE_PATH
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-management-test-"))
    const pageDirectory = path.join(root, "web", "pages")
    fs.mkdirSync(pageDirectory, { recursive: true })
    fs.copyFileSync(path.join(repositoryRoot, "web", "pages", "management.html"), path.join(pageDirectory, "management.html"))

    let app
    let database
    try {
        process.chdir(root)
        process.env.MANAGEMENT_STATE_FILE = path.join(root, ".management", "state.json")
        process.env.DATABASE_PATH = path.join(root, "custom-data", "starpoint-cn.sqlite")
        process.env.MANAGEMENT_ACCESS_DATABASE_PATH = path.join(root, ".management", "control.db")
        const managementPlugin = require(managementPluginPath).default
        const data = require(dataModulePath)
        const initializeDatabase = require(databaseInitializerModulePath).default
        const { SessionType } = require(dataTypesPath)
        const { CN_CONTENT_BASELINE_ISO, ManagementStore, managementStore } = require(managementStoreModulePath)
        const { managementAccessStore } = require(managementAccessModulePath)
        const battleSettlementState = require(battleSettlementStateModulePath)
        const { matchmakingStore } = require(matchmakingStoreModulePath)
        database = require(databaseModulePath).default(0)

        app = Fastify({ logger: false })
        app.register(managementPlugin, { prefix: "/manage" })
        await app.ready()
        await verifyAuthentication(app)
        const fixtures = await verifyAccountAdministration(app, data, SessionType)
        verifyActiveSlotBackfill(database, initializeDatabase, data, fixtures)
        await verifyUserAccess(app, managementAccessStore, data, battleSettlementState, matchmakingStore, fixtures)
        await verifyMailAdministration(app, fixtures)
        if (process.argv.includes("--personal-service-save-sync")) {
            await app.listen({ host: "127.0.0.1", port: 0 })
            await verifyPersonalServiceSaveSync(app, root, managementAccessStore, playerCredentials)
            await verifyThreeInstancePortableSave(app, root, {
                managementToken: adminToken,
                playerId: fixtures.first.player.id,
                viewerId: fixtures.first.activeViewerId,
            })
        }
        await verifyDefaultCnContentTime(managementStore, ManagementStore, CN_CONTENT_BASELINE_ISO)
        await verifyVirtualTimePersistence(app, managementStore, ManagementStore)
        const backupFixture = await prepareBackupRestoreFixture(data, managementStore)
        await app.close()
        app = undefined
        database.close()
        database = undefined
        await verifyBackupRestore(managementStore, backupFixture)
        console.log("Management API test passed.")
    } finally {
        if (app !== undefined) await app.close()
        if (database !== undefined && database.open) database.close()
        process.chdir(originalCwd)
        if (originalToken === undefined) delete process.env.MANAGEMENT_ADMIN_TOKEN
        else process.env.MANAGEMENT_ADMIN_TOKEN = originalToken
        if (originalStateFile === undefined) delete process.env.MANAGEMENT_STATE_FILE
        else process.env.MANAGEMENT_STATE_FILE = originalStateFile
        if (originalDatabasePath === undefined) delete process.env.DATABASE_PATH
        else process.env.DATABASE_PATH = originalDatabasePath
        if (originalAccessDatabasePath === undefined) delete process.env.MANAGEMENT_ACCESS_DATABASE_PATH
        else process.env.MANAGEMENT_ACCESS_DATABASE_PATH = originalAccessDatabasePath
        fs.rmSync(root, { recursive: true, force: true })
    }
}
// //// /在临时数据库上装配并关闭管理插件 ////

run().catch((error) => {
    console.error(error)
    process.exitCode = 1
})
