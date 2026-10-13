// audience: internal
// # character-reference
// 客户端人物槽位只引用角色主表中的真实编号, 空成员使用 null.

import { getCharacterDataSync } from "./assets"

// //// 将失效编号转换为客户端空人物槽位 [@x380kkm 2026-09-28] ////
export function characterReference(value: number | null | undefined): number | null {
    return value !== null && value !== undefined && Number.isSafeInteger(value)
        && value > 0 && getCharacterDataSync(value) !== null ? value : null
}
