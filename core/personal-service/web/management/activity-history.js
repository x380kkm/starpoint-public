// audience: internal
// # personal-service-activity-history-view
// 活动详情按选定存档展示各轮成绩, 领奖和投递记录. 重置以当前轮次和用户确认提交.

const kindLabels = { statistics: "战斗结算", summary: "排名统计", claim: "奖励领取", delivery: "邮件投递", exchange: "商店兑换", reset: "活动重置", vote: "角色投票" }
const missionRewardLabels = ["星导石", "道具", "装备", "玛纳", "角色", "经验", "称号"]

// //// 将活动记录转换为可读摘要 [@x380kkm 2026-09-21] ////
export function historySummary(entry) {
    const details = entry.details ?? {}
    if (entry.kind === "statistics") {
        const result = details.is_accomplished ? "通关" : "未通关"
        const best = details.best_record
        const bestText = best ? `, ${bestRecordSummary(best)}` : ""
        return `${details.quest_name || `关卡 ${details.quest_id}`} - ${result}, ${seconds(details.elapsed_time_ms)}, 得分 ${details.score ?? 0}${bestText}`
    }
    if (entry.kind === "summary") {
        if (details.best_record) return `最佳 ${seconds(details.best_record.elapsed_time_ms)}, 排名 ${Number(details.rank_percentage ?? 100).toFixed(2)}%`
        if (Array.isArray(details.records)) return `${details.records.length} 个关卡, 总最佳分 ${details.records.reduce((sum,record) => sum + Number(record.best_score ?? 0), 0)}`
        return "活动统计已记录, 展开查看成绩明细."
    }
    if (entry.kind === "reset") return `第 ${details.previous_round + 1} 轮结束, 开始第 ${details.next_round + 1} 轮`
    if (entry.kind === "vote") return `投票角色关键词 ${details.keyword_id}`
    if (entry.kind === "claim") {
        const receipt = details.receipt ?? {}
        const rewards = (Array.isArray(receipt.rewards) ? receipt.rewards : []).map((reward) => {
            const id = reward.item_id ?? reward.equipment_id ?? reward.character_id ?? reward.degree_id
            return `${missionRewardLabels[reward.kind] ?? "奖励"}${id ? ` ${id}` : ""} x${reward.amount ?? 1}`
        })
        if (receipt.tier_id) rewards.unshift(`排名档位 ${receipt.tier_id}`)
        return rewards.join(", ") || `领取凭据 ${details.receipt_id}`
    }
    if (entry.kind === "delivery") return `${details.title ?? "活动奖励"} - ${entry.received_at ? `已领取 (${formatTime(entry.received_at)})` : "已投递, 待领取"}`
    if (entry.kind === "exchange") return `商店 ${details.request?.shop_type ?? ""}, 获得 ${(details.rewards ?? []).map((reward) => `${reward.type_id ?? reward.kind} x${reward.number}`).join(", ")}`
    return ""
}

// //// 按成绩结构显示通关竞速和关卡最佳记录 [@x380kkm 2026-09-28] ////
function bestRecordSummary(best) {
    if (typeof best.is_accomplished === "boolean" && best.elapsed_time_ms !== undefined) {
        const result = best.is_accomplished ? seconds(best.elapsed_time_ms) : "未通关"
        return `最佳 ${result}, 得分 ${best.score ?? 0}`
    }
    return `最佳 ${seconds(best.best_elapsed_time_ms ?? best.elapsed_time_ms)}, 最高分 ${best.high_score ?? 0}`
}

function seconds(value) { return value !== null && value !== undefined && Number.isFinite(Number(value)) ? `${(Number(value) / 1000).toFixed(3)} 秒` : "无成绩" }
function formatTime(value) { return new Date(value * 1000).toLocaleString("zh-CN", { hour12: false }) }

// //// 管理活动历史读取和明确的重置操作 [@x380kkm 2026-09-21] ////
export function createActivityHistory({ requestApi, runAction, document = globalThis.document, confirm = globalThis.confirm, prefix = "activity-history", onReset }) {
    const controls = Object.fromEntries(["save", "state", "list", "refresh", "more", "reset"].map((key) => [key, document.querySelector(`#${prefix}-${key}`)]))
    let activity = null
    let round = null
    let entries = []
    let nextBeforeId = null
    let generation = 0
    let canReset = false

    function clear() {
        generation += 1
        round = null
        canReset = false
        entries = []
        nextBeforeId = null
        controls.list.replaceChildren()
        controls.reset.disabled = true
        controls.more.hidden = true
        controls.state.textContent = "选择存档后查看活动记录."
    }

    function render() {
        controls.list.replaceChildren()
        for (const entry of entries) {
            const card = document.createElement("details")
            card.className = "form-drawer"
            const summary = document.createElement("summary")
            summary.textContent = `第 ${entry.participation_round + 1} 轮 · ${kindLabels[entry.kind] ?? entry.kind} · ${formatTime(entry.created_at)}`
            const description = document.createElement("p")
            description.textContent = historySummary(entry)
            const raw = document.createElement("pre")
            raw.className = "activity-history-data"
            raw.textContent = JSON.stringify(entry.details, null, 2)
            card.append(summary, description, raw)
            controls.list.append(card)
        }
        controls.more.hidden = nextBeforeId === null
        controls.state.textContent = `当前第 ${round + 1} 轮, 显示 ${entries.length} 条记录.`
    }

    async function load(append = false) {
        if (!activity || !controls.save.value) { clear(); return }
        const selectedActivity = activity.activity_id
        const slotId = controls.save.value
        const requestGeneration = ++generation
        canReset = false
        controls.reset.disabled = true
        const query = append && nextBeforeId ? `?before_id=${nextBeforeId}` : ""
        const payload = await requestApi(`/v1/local-saves/${slotId}/activities/${encodeURIComponent(selectedActivity)}/history${query}`)
        if (requestGeneration !== generation) return
        round = payload.participation_round
        entries = append ? [...entries, ...payload.entries] : payload.entries
        nextBeforeId = payload.next_before_id
        canReset = payload.can_reset === true
        controls.reset.disabled = !canReset
        render()
    }

    async function reset() {
        if (!canReset || round === null || !activity || !controls.save.value) return
        const slotName = controls.save.selectedOptions[0]?.textContent ?? controls.save.value
        const selectedActivity = activity.activity_id
        const slotId = controls.save.value
        const expectedRound = round
        if (!confirm(`重置存档 "${slotName}" 的活动 "${activity.name}" (第 ${expectedRound + 1} 轮)?\n成绩, 领奖资格和活动商店限购会重新开始. 历史记录, 邮件和已获得资产会保留.\n完成后请返回游戏标题画面重新加载存档.`)) return
        await requestApi(`/v1/local-saves/${slotId}/activities/${encodeURIComponent(selectedActivity)}/reset`, {
            method: "POST", body: { expected_round: expectedRound },
        })
        await load()
        await onReset?.()
        return "活动已重置, 请重新加载游戏存档后参加."
    }

    function renderSaves(slots, selected) {
        controls.save.replaceChildren()
        const empty = document.createElement("option")
        empty.value = ""
        empty.textContent = "选择存档"
        controls.save.append(empty)
        for (const slot of slots) {
            const option = document.createElement("option")
            option.value = String(slot.id)
            option.textContent = `${slot.name} (#${slot.id})`
            controls.save.append(option)
        }
        controls.save.value = slots.some(slot => String(slot.id) === selected) ? selected : ""
        clear()
    }

    controls.save.addEventListener("change", () => { clear(); return runAction(controls.refresh, () => load()) })
    controls.refresh.addEventListener("click", () => runAction(controls.refresh, () => load()))
    controls.more.addEventListener("click", () => runAction(controls.more, () => load(true)))
    controls.reset.addEventListener("click", async () => {
        await runAction(controls.reset, reset)
        controls.reset.disabled = !canReset
    })
    clear()
    return {
        selectActivity(value) { activity = value; clear(); return runAction(controls.refresh, () => load()) },
        selectContext({ activity: value, slot }) {
            activity = value
            renderSaves(slot ? [slot] : [], slot ? String(slot.id) : "")
            return runAction(controls.refresh, () => load())
        },
        setSaves(slots) {
            renderSaves(slots, controls.save.value)
            return runAction(controls.refresh, () => load())
        },
    }
}
