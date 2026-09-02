// audience: internal
// # account-administration
// 此模块为管理 API 提供脱敏账号摘要和会话控制.
// 所有操作先验证内部账号 ID, 且列表响应不返回登录标识或会话 token.

import {
    AccountPageRequest,
    deleteAccountSessions,
    generateViewerIdSession,
    getAccount,
    getAccountPlayers,
    getActivePlayerId,
    getAccountSessions,
    listAccountsPage,
} from "../data/wdfpData"
import { Account, Session, SessionType } from "../data/types"

export interface AccountSessionCounts {
    zat: number
    zrt: number
    viewer: number
}

export interface AccountOverview {
    id: number
    appId: string
    status: string
    firstLoginTime: string
    registrationTime: string
    lastLoginTime: string
    playerIds: number[]
    activePlayerId: number | null
    sessionCounts: AccountSessionCounts
}

export interface AccountOverviewPage extends AccountPageRequest {
    accounts: AccountOverview[]
    total: number
}

export interface RevokedAccountSessions {
    accountId: number
    revokedSessions: number
}

export interface RotatedViewerId {
    accountId: number
    viewerId: string
}

export class AccountNotFoundError extends Error {
    constructor(accountId: number) {
        super(`Account does not exist: ${accountId}`)
        this.name = "AccountNotFoundError"
    }
}

// //// 生成不包含认证凭据的账号摘要 [@x380kkm 2026-07-22] ////
function countSessions(sessions: Session[]): AccountSessionCounts {
    const counts: AccountSessionCounts = { zat: 0, zrt: 0, viewer: 0 }
    for (const session of sessions) {
        switch (session.type) {
            case SessionType.ZAT:
                counts.zat += 1
                break
            case SessionType.ZRT:
                counts.zrt += 1
                break
            case SessionType.VIEWER:
                counts.viewer += 1
                break
        }
    }
    return counts
}

async function createAccountOverview(account: Account): Promise<AccountOverview> {
    const [playerIds, activePlayerId, sessions] = await Promise.all([
        getAccountPlayers(account.id),
        getActivePlayerId(account.id),
        getAccountSessions(account.id),
    ])
    return {
        id: account.id,
        appId: account.appId,
        status: account.status,
        firstLoginTime: account.firstLoginTime.toISOString(),
        registrationTime: account.regTime.toISOString(),
        lastLoginTime: account.lastLoginTime.toISOString(),
        playerIds,
        activePlayerId,
        sessionCounts: countSessions(sessions),
    }
}
// //// /生成不包含认证凭据的账号摘要 ////

// //// 分页读取脱敏账号摘要 [@x380kkm 2026-07-22] ////
export async function listAccountOverviews(request: AccountPageRequest): Promise<AccountOverviewPage> {
    const page = await listAccountsPage(request)
    return {
        accounts: await Promise.all(page.accounts.map(createAccountOverview)),
        total: page.total,
        limit: request.limit,
        offset: request.offset,
    }
}
// //// /分页读取脱敏账号摘要 ////

// //// 撤销单个账号的全部会话 [@x380kkm 2026-07-22] ////
export async function revokeAccountSessions(accountId: number): Promise<RevokedAccountSessions> {
    const account = await getAccount(accountId)
    if (account === null) throw new AccountNotFoundError(accountId)

    const sessions = await getAccountSessions(accountId)
    await deleteAccountSessions(accountId)
    return { accountId, revokedSessions: sessions.length }
}
// //// /撤销单个账号的全部会话 ////

// //// 为单个账号轮换 Viewer ID [@x380kkm 2026-07-22] ////
export async function rotateAccountViewerId(accountId: number): Promise<RotatedViewerId> {
    const account = await getAccount(accountId)
    if (account === null) throw new AccountNotFoundError(accountId)

    const session = await generateViewerIdSession(accountId)
    return { accountId, viewerId: session.token }
}
// //// /为单个账号轮换 Viewer ID ////
