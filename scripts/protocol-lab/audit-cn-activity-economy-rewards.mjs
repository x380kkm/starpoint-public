// audience: internal
// # audit-cn-activity-economy-rewards
// 关卡奖励来源遵循个人服务的奖励类型, 抽选资格和追加范围编码.
// 静态来源表示存在正概率奖励, 玩家前置通关和活动日历由运行时判定.

const CATEGORIES_BY_KIND = [[1], [4], [2], [6], [14], [7], [10], [13], [11], [18], [19], [15], [6, 14, 13, 20], [20], [21], [22], [23], [24], [25], [26], [27]]
const BATTLE_ASSET = "core/personal-service/assets/cn-single-battle.json"
const RULES_ASSET = "core/personal-service/assets/cn-battle-reward-rules.json"
const ADDITIONAL_ASSET = "core/personal-service/assets/cn-additional-battle-rewards.json"
const PERIODIC_ASSET = "core/personal-service/assets/cn-periodic-battle-rewards.json"

// //// 按客户端范围编码匹配追加掉落关卡 [@x380kkm 2026-09-29] ////
export function matchesAdditionalQuest(relation, quest) {
    if (relation.quest_parts.some((part) => Array.isArray(part) && part.length === 0)) return false
    const kind = relation.quest_kind
    if (kind === null || kind === undefined) return true
    if (!(CATEGORIES_BY_KIND[kind] ?? []).includes(quest.category)) return false
    const id = quest.quest_id
    const parts = [0, 1, 2].includes(kind)
        ? [Math.floor(id / 1_000_000), Math.floor(id / 1_000) % 1_000, id % 1_000]
        : [11, 12].includes(kind) ? [0, 0, id] : [Math.floor(id / 1_000), 0, id % 1_000]
    return relation.quest_parts.every((expected, index) => expected === null || expected === undefined || expected.includes(parts[index]))
}

// //// 将奖励解析成可领取的道具编号 [@x380kkm 2026-09-29] ////
function itemReward(reward, elementRarity, element, elementMap) {
    if (!reward || ![0, 6, 7].includes(reward.type) || !(reward.count > 0)) return null
    if (elementRarity === undefined || elementRarity === null) return reward.id ?? null
    const mapKind = reward.type === 6 ? 1 : 2
    const enemyElement = [3, 0, 1, 2, 5, 4][element ?? 0]
    return elementMap[mapKind]?.[elementRarity]?.[enemyElement]?.[0] ?? null
}

// //// 保存成本货币的关卡来源及源码锚点 [@x380kkm 2026-09-29] ////
function sourceRecorder(context, questKey, activityId, base = {}) {
    const quest = context.battle.quests[questKey]
    return (reward, anchor, kind, elementRarity = null, detail = {}) => {
        const itemId = itemReward(reward, elementRarity, quest?.element, context.rules.reward_element_map)
        if (!context.currencyIds.has(itemId)) return
        context.addSource(itemId, { activity_id: activityId ?? null, quest_key: questKey, kind, anchor, ...base, ...detail })
    }
}

// //// 读取普通和稀有抽选的正概率奖励 [@x380kkm 2026-09-29] ////
function scoreSources(definition, rareGroups, record, anchor, rareAnchor, issues) {
    if (definition.score_reward_group_id === null || definition.score_reward_group_id === undefined) return
    const draws = Math.max(definition.score_reward_count ?? 0, ...(definition.score_reward_counts ?? [0]))
    for (let index = 0; index < (definition.score_rewards ?? []).length; index += 1) {
        const reward = definition.score_rewards[index]
        const rewardAnchor = `${anchor}/score_rewards/${index}`
        if (reward.type === 0 && draws > 0 && reward.field5 > 0) {
            record({ ...reward, type: reward.reward_type }, rewardAnchor, "score_drop", reward.element_rarity)
        }
        if (reward.type !== 1 || !(reward.rarity > 0)) continue
        const group = rareGroups[reward.id]
        if (!group) {
            issues.push({ kind: "missing_rare_reward_group", group_id: reward.id, anchor: rewardAnchor })
            continue
        }
        let cumulative = 0
        for (let rareIndex = 0; rareIndex < group.length; rareIndex += 1) {
            const rare = group[rareIndex]
            if (rare.rarity > 0 && cumulative < 1) record(rare, `${rareAnchor}/${reward.id}/${rareIndex}`, "rare_drop", rare.element_rarity)
            cumulative += rare.rarity ?? 0
        }
    }
}

// //// 汇总基础掉落, 首通奖励和阶段奖励 [@x380kkm 2026-09-29] ////
export function recordBattleSources(context) {
    for (const [key, quest] of Object.entries(context.battle.quests)) {
        const activityId = quest.activity_id ?? context.windows.quests[key]?.activity_id
        const record = sourceRecorder(context, key, activityId)
        const anchor = `${BATTLE_ASSET}#/quests/${key}`
        record(quest.clear_reward, `${anchor}/clear_reward`, "first_clear")
        record(quest.s_plus_reward, `${anchor}/s_plus_reward`, "first_s_plus")
        scoreSources(quest, context.battle.rare_reward_groups, record, anchor, `${BATTLE_ASSET}#/rare_reward_groups`, context.issues)
        for (const [index, tier] of (quest.score_attack_border_rewards ?? []).entries()) {
            for (const [rewardIndex, reward] of tier.rewards.entries()) {
                if (reward.kind === 1) record({ type: 0, id: reward.item_id, count: reward.amount }, `${anchor}/score_attack_border_rewards/${index}/rewards/${rewardIndex}`, "score_threshold")
            }
        }
        const phaseQuest = context.rules.phase_quests[key]
        for (const [index, phase] of (phaseQuest?.phases ?? []).entries()) {
            scoreSources(phase, phaseQuest.rare_reward_groups, record, `${RULES_ASSET}#/phase_quests/${key}/phases/${index}`, `${RULES_ASSET}#/phase_quests/${key}/rare_reward_groups`, context.issues)
        }
    }
}

// //// 汇总跨关卡追加掉落的实际匹配来源 [@x380kkm 2026-09-29] ////
export function recordAdditionalSources(context) {
    const eligible = Object.entries(context.battle.quests).filter(([key, quest]) => ![3, 9, 16, 20].includes(quest.category) && context.additional.quest_enemy_levels[key] !== undefined)
    for (const [eventIndex, event] of context.additional.events.entries()) {
        const activityId = `collect-item:${event.id}`
        let matchedQuests = 0
        for (const [key, quest] of eligible) {
            const level = context.additional.quest_enemy_levels[key]
            const record = sourceRecorder(context, key, activityId, { quest_activity_id: quest.activity_id ?? context.windows.quests[key]?.activity_id ?? null, required_quest: event.required_quest ?? null })
            let matched = false
            for (const [relationIndex, relation] of event.relations.entries()) {
                if (!matchesAdditionalQuest(relation, quest)) continue
                const thresholds = context.additional.reward_relations[relation.reward_relation_id]
                if (!thresholds) {
                    context.issues.push({ kind: "missing_additional_relation", activity_id: activityId, anchor: `${ADDITIONAL_ASSET}#/events/${eventIndex}/relations/${relationIndex}` })
                    continue
                }
                for (const threshold of thresholds.filter((entry) => entry.minimum_enemy_level <= level)) {
                    const group = context.additional.reward_groups[threshold.group_id]
                    if (!group) {
                        context.issues.push({ kind: "missing_additional_group", activity_id: activityId, group_id: threshold.group_id, anchor: `${ADDITIONAL_ASSET}#/reward_relations/${relation.reward_relation_id}` })
                        continue
                    }
                    matched = true
                    for (const [index, entry] of group.entries()) {
                        if (entry.weight > 0) record(entry.reward, `${ADDITIONAL_ASSET}#/reward_groups/${threshold.group_id}/${index}`, "additional_drop", entry.element_rarity, { relation_id: relation.reward_relation_id, minimum_enemy_level: threshold.minimum_enemy_level })
                    }
                }
            }
            if (matched) matchedQuests += 1
        }
        context.additionalCoverage.push({ activity_id: activityId, matched_quest_count: matchedQuests })
    }
}

// //// 汇总有可恢复点数的周期奖励来源 [@x380kkm 2026-09-29] ////
export function recordPeriodicSources(context) {
    for (const [key, quest] of Object.entries(context.periodic.quests)) {
        const point = context.periodic.points[quest.point_id]
        if (!point || !(quest.slots > 0) || !(point.max_point > 0)) continue
        const group = context.periodic.reward_groups[quest.group_id]
        if (!group) {
            context.issues.push({ kind: "missing_periodic_group", activity_id: point.activity_id, anchor: `${PERIODIC_ASSET}#/quests/${key}` })
            continue
        }
        const record = sourceRecorder(context, key, point.activity_id, { point_id: quest.point_id })
        for (const [index, reward] of group.entries()) {
            if (reward.probability > 0) record(reward.reward, `${PERIODIC_ASSET}#/reward_groups/${quest.group_id}/${index}`, "periodic_drop", reward.element_rarity)
        }
    }
}
