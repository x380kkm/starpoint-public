// audience: internal
// # multiplayer-session-scenario-differential
//
// 此模块比较 TypeScript 会话服务和 Rust 个人服务的多人状态机场景.

import { existsSync } from "node:fs"
import path from "node:path"
import { compareFacts } from "./multiplayer-protocol-differential-lib.mjs"
import { parseRustSession } from "./multiplayer-rust-session-scenarios.mjs"
import { parseTypeScriptSession } from "./multiplayer-typescript-session-scenarios.mjs"

// //// 生成会话场景差分 [@x380kkm 2026-08-24] ////
export function collectSessionScenarioDifferential(repositoryRoot) {
    const sessionPath = path.join(repositoryRoot, "src", "multiplayer", "sessionServer.ts")
    if (!existsSync(sessionPath)) return null
    const reference = parseTypeScriptSession(repositoryRoot)
    const local = parseRustSession(repositoryRoot)
    return {
        reference: reference.required,
        local: local.required,
        policyComparison: compareFacts(reference.policy.facts, local.policy.facts),
    }
}
// //// /生成会话场景差分 ////
