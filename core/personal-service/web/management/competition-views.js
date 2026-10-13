// audience: internal
// # personal-service-competition-views
// 竞赛页面使用真实成绩, 奖励目录和期别统计, 文本通过 DOM 节点呈现.

export const competitionFamilies = { ranking: "竞速挑战", rush: "狂热激战", raid: "全服讨伐" }
const availabilityLabels = { open: "开放中", ended: "已结束", disabled: "已关闭", not_started: "未开始", closed: "已关闭" }

// //// 显示当前参赛记录与活动范围 [@x380kkm 2026-09-28] ////
export function competitionSummary(entry) {
    const summary = entry.summary ?? {}
    if (entry.family === "ranking") {
        const record = summary.best_record
        if (!record) return "尚未参赛. 完成挑战后可按成绩或所选档位发奖."
        return `${record.is_accomplished ? `最佳 ${(Number(record.elapsed_time_ms) / 1000).toFixed(3)} 秒` : "尚未通关"}, 得分 ${record.score ?? 0}, 排名 ${Number(summary.rank_percentage ?? 100).toFixed(2)}%.`
    }
    if (entry.family === "rush") {
        return entry.has_record
            ? `最佳第 ${summary.best_round ?? 0} 轮, ${(Number(summary.elapsed_time_ms ?? 0) / 1000).toFixed(3)} 秒, 当前第 ${summary.rank_number ?? entry.suggested_rank_number ?? 1} 名.`
            : "尚无无限战成绩. 完成挑战后可结算排名奖励."
    }
    return `全服累计讨伐 ${entry.global_state?.total_kill_count ?? 0} 次, 此存档已发放至 ${summary.received_up_to ?? 0} 次.`
}

export function competitionPeriod(entry) {
    const availability = entry.availability ?? {}
    const status = availabilityLabels[availability.status] ?? "按活动时间开放"
    const start = entry.period_start_ms
    const period = start === null || start === undefined ? "原始期别" : new Date(start).toLocaleString("zh-CN", { hour12: false })
    return `${competitionFamilies[entry.family] ?? entry.family} / ${status} / 第 ${entry.participation_round + 1} 轮 / ${period}`
}

// //// 从当前输入选择目录中的奖励档位 [@x380kkm 2026-09-28] ////
export function selectedCompetitionTiers(entry, tierId, rankNumber) {
    const tiers = entry.tiers ?? []
    if (entry.family === "ranking") {
        const selected = tierId === "" ? entry.suggested_tier_id : Number(tierId)
        return tiers.filter(tier => tier.id === selected)
    }
    if (entry.family === "rush") {
        const rank = Number(rankNumber)
        return tiers.filter(tier => Number.isSafeInteger(rank) && rank >= tier.rank_from && rank <= tier.rank_to)
    }
    return tiers.filter(tier => Number(tier.pending_count ?? 0) > 0)
}

// //// 展示奖励预览并保留每个阈值的实际金额 [@x380kkm 2026-09-28] ////
export function renderCompetitionRewards(document, container, tiers) {
    container.replaceChildren()
    for (const tier of tiers) {
        const card = document.createElement("article")
        card.className = "item-card"
        const title = document.createElement("strong")
        const count = tier.pending_count ?? 1
        title.textContent = `${tier.label ?? `奖励档位 ${tier.id}`}${count > 1 ? ` / ${count} 次奖励` : ""}`
        const rewards = document.createElement("p")
        rewards.className = "meta"
        rewards.textContent = (tier.rewards ?? []).map(reward => `${reward.label ?? `奖励 ${reward.id ?? reward.kind}`} x${reward.amount * count}`).join(", ") || "此档位没有额外奖励."
        card.append(title, rewards)
        container.append(card)
    }
    if (!tiers.length) {
        const empty = document.createElement("p")
        empty.className = "empty-state"
        empty.textContent = "当前没有满足条件的新奖励."
        container.append(empty)
    }
}
