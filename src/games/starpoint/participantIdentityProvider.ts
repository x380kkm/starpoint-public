// audience: internal
// # starpoint-participant-identity-provider
// 此模块把 StarPoint viewer session 和玩家数据转换为多人服务使用的账号身份.
// 账号至少绑定一个仍存在的玩家时才具备参加联机的身份.

import { getAccountPlayers, getPlayerSync, getSession } from "../../data/wdfpData"
import type { ParticipantIdentityProvider } from "../../multiplayer/sessionDependencies"

class StarpointParticipantIdentityProvider implements ParticipantIdentityProvider {
    // //// 验证 viewer 归属和玩家数据 [@x380kkm 2026-07-22] ////
    async isPlayableParticipant(viewerId: number, accountId: number): Promise<boolean> {
        const session = await getSession(viewerId.toString())
        if (session === null || session.accountId !== accountId) return false
        const playerIds = await getAccountPlayers(session.accountId)
        return playerIds.length > 0 && getPlayerSync(playerIds[0]) !== null
    }
    // //// /验证 viewer 归属和玩家数据 ////
}

export const starpointParticipantIdentityProvider = new StarpointParticipantIdentityProvider()
