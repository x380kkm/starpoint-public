// audience: internal
// # test-cn-gacha
//
// 该脚本用真实 CN MessagePack 请求验证管理员资源邮件, 单抽, 十连, 角色兑换和重启后的扭蛋持久化.

import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { pack, unpack } from "msgpackr"
import {
    requestJson,
    signupCn,
    startNodeServer,
    stopChildProcess,
} from "./loopback-test-services.js"

// //// 发送 CN MessagePack 请求并返回脱包结果 [@x380kkm 2026-08-14] ////
async function requestCn(baseUrl, request) {
    const response = await fetch(`${baseUrl}${request.path}`, {
        method: request.method,
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: Buffer.from(pack(request.payload)).toString("base64"),
        signal: AbortSignal.timeout(10_000),
    })
    const text = await response.text()
    assert.equal(
        response.status,
        request.expectedStatus,
        `${request.method} ${request.path} returned ${response.status}: ${text.slice(0, 512)}`,
    )
    const contentType = response.headers.get("content-type") ?? ""
    if (contentType.includes("application/x-msgpack")) {
        return { body: unpack(Buffer.from(text, "base64")), contentType }
    }
    if (request.expectedStatus === 200) assert.fail(`${request.method} ${request.path} did not return MessagePack.`)
    return { body: text === "" ? null : JSON.parse(text), contentType }
}
// //// /发送 CN MessagePack 请求并返回脱包结果 ////

// //// 读取 CN 玩家状态 [@x380kkm 2026-08-14] ////
function loadCnData(baseUrl, viewerId, deviceId) {
    return requestCn(baseUrl, {
        method: "POST",
        path: "/api/index.php/load",
        payload: {
            device_id: deviceId,
            device_token: "",
            keychain: viewerId,
            graphics_device_name: "CN gacha test",
            platform_os_version: "Android test",
            storage_directory_path: "/data/user/0/com.leiting.wf",
            viewer_id: viewerId,
        },
        expectedStatus: 200,
    })
}
// //// /读取 CN 玩家状态 ////

// //// 验证角色扭蛋演出字段 [@x380kkm 2026-08-22] ////
function assertCharacterDraw(draw) {
    assert.equal(Number.isSafeInteger(draw.character_id), true)
    assert.equal(typeof draw.movie_id, "string")
    assert.notEqual(draw.movie_id, "")
    assert.equal(Number.isSafeInteger(draw.seed), true)
    assert.equal(draw.entry_count, 1)
}
// //// /验证角色扭蛋演出字段 ////

// //// 验证 CN 扭蛋资源、抽取和持久化闭环 [@x380kkm 2026-08-14] ////
async function verifyCnGacha() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-gacha-"))
    let server = await startNodeServer(root, "cn-gacha-test-management-token")
    try {
        const deviceId = Number(`${Date.now()}`.slice(-8))
        const signup = await signupCn(server.baseUrl, deviceId, `cn-gacha-${process.pid}`)
        const viewerId = signup.data_headers.viewer_id
        const saves = await requestJson(server.baseUrl, {
            method: "GET",
            path: "/manage/api/saves",
            token: server.managementToken,
            expectedStatus: 200,
        })
        assert.equal(saves.body.players.length, 1)
        const playerId = saves.body.players[0].id

        const single = await requestCn(server.baseUrl, {
            method: "POST",
            path: "/api/index.php/gacha/exec",
            payload: {
                api_count: 1,
                payment_type: 1,
                number_of_exec: 1,
                viewer_id: viewerId,
                gacha_id: 1,
                type: 1,
            },
            expectedStatus: 200,
        })
        assert.equal(single.body.data_headers.result_code, 1)
        assert.equal(single.body.data.user_info.free_vmoney, 0)
        assert.equal(single.body.data.draw.length, 1)
        assertCharacterDraw(single.body.data.draw[0])
        assert.equal(single.body.data.gacha_info_list[0].gacha_exchange_point, 1)

        const rejectedTicketDraw = await requestCn(server.baseUrl, {
            method: "POST",
            path: "/api/index.php/gacha/exec",
            payload: {
                api_count: 1,
                payment_type: 3,
                number_of_exec: 1,
                viewer_id: viewerId,
                gacha_id: 1,
                type: 10,
            },
            expectedStatus: 400,
        })
        assert.equal(rejectedTicketDraw.body.message, "Not enough tickets.")
        const afterRejectedTicketDraw = await loadCnData(server.baseUrl, viewerId, deviceId)
        assert.equal(afterRejectedTicketDraw.body.data.user_info.free_vmoney, 0)
        assert.equal(afterRejectedTicketDraw.body.data.gacha_info_list.find((entry) => Number(entry.gacha_id) === 1).gacha_exchange_point, 1)

        const mail = await requestJson(server.baseUrl, {
            method: "POST",
            path: "/manage/api/mails",
            token: server.managementToken,
            payload: {
                playerId,
                title: "CN gacha resource test",
                body: "Synthetic test resources.",
                sender: "Starpoint test",
                rewards: { freeVmoney: 39000 },
            },
            expectedStatus: 200,
        })
        const mailIndex = await requestCn(server.baseUrl, {
            method: "POST",
            path: "/api/index.php/mail/index",
            payload: { viewer_id: viewerId, current_page: 1 },
            expectedStatus: 200,
        })
        assert.equal(mailIndex.body.data.mail.length, 1)
        assert.equal(mailIndex.body.data.mail[0].id, mail.body.id)

        const mailReceive = await requestCn(server.baseUrl, {
            method: "POST",
            path: "/api/index.php/mail/receive",
            payload: { viewer_id: viewerId, mail_id: mail.body.id },
            expectedStatus: 200,
        })
        assert.equal(mailReceive.body.data.user_info.free_vmoney, 39000)

        const multi = await requestCn(server.baseUrl, {
            method: "POST",
            path: "/api/index.php/gacha/exec",
            payload: {
                api_count: 1,
                payment_type: 1,
                number_of_exec: 1,
                viewer_id: viewerId,
                gacha_id: 1,
                type: 2,
            },
            expectedStatus: 200,
        })
        assert.equal(multi.body.data_headers.result_code, 1)
        assert.equal(multi.body.data.user_info.free_vmoney, 37500)
        assert.equal(multi.body.data.draw.length, 10)
        multi.body.data.draw.forEach(assertCharacterDraw)
        assert.equal(multi.body.data.gacha_info_list[0].gacha_exchange_point, 11)

        let lastMulti = multi
        for (let index = 0; index < 25; index++) {
            lastMulti = await requestCn(server.baseUrl, {
                method: "POST",
                path: "/api/index.php/gacha/exec",
                payload: {
                    api_count: 1,
                    payment_type: 1,
                    number_of_exec: 1,
                    viewer_id: viewerId,
                    gacha_id: 1,
                    type: 2,
                },
                expectedStatus: 200,
            })
        }
        assert.equal(lastMulti.body.data.user_info.free_vmoney, 0)
        lastMulti.body.data.draw.forEach(assertCharacterDraw)
        assert.equal(lastMulti.body.data.gacha_info_list[0].gacha_exchange_point, 261)

        const exchange = await requestCn(server.baseUrl, {
            method: "POST",
            path: "/api/index.php/gacha/exchange_character",
            payload: { api_count: 1, character_id: 111001, gacha_id: 1, viewer_id: viewerId },
            expectedStatus: 200,
        })
        assert.equal(exchange.body.data_headers.result_code, 1)
        assert.equal(exchange.body.data.character_list.length, 1)
        assert.equal(exchange.body.data.gacha_info_list[0].gacha_exchange_point, 11)

        await stopChildProcess(server.process, "CN gacha test server before restart")
        server = await startNodeServer(root, "cn-gacha-test-management-token")
        const reloaded = await loadCnData(server.baseUrl, viewerId, deviceId)
        assert.equal(reloaded.body.data.user_info.free_vmoney, 0)
        assert.equal(reloaded.body.data.gacha_info_list.find((entry) => Number(entry.gacha_id) === 1).gacha_exchange_point, 11)
        assert.equal(Object.keys(reloaded.body.data.user_character_list).length > 0, true)
    } finally {
        await stopChildProcess(server.process, "CN gacha test server")
        fs.rmSync(root, { recursive: true, force: true })
    }
}
// //// /验证 CN 扭蛋资源、抽取和持久化闭环 ////

verifyCnGacha()
    .then(() => console.log("CN gacha contract test passed."))
    .catch((error) => {
        console.error(error)
        process.exitCode = 1
    })
