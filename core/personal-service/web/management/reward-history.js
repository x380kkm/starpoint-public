// audience: internal
// # personal-service-reward-history-view
// 此视图按存档分页显示到账资产, 已发放商品和邮件投递状态. 所有展示内容使用文本节点.

const sourceLabels = { product: "商品领取", mail: "邮件领取", login: "登录与订阅奖励", mission: "任务与活动奖励", exchange: "商店兑换", gacha: "扭蛋奖励", reward: "奖励领取" }
const assetLabels = { 1: "道具", 3: "付费星导石", 4: "免费星导石", 5: "角色", 6: "装备", 7: "星之碎片", 8: "玛纳", 9: "经验池", 10: "羁绊之证", 11: "Boss Boost 点数", 12: "Boost 点数", 15: "段位点数" }
const deliveryLabels = { received: "已领取到账", pending: "已投递, 待领取", expired: "已过期, 未领取" }
const entitlementLabels = { growth_fund: "成长基金资格", pass_card: "通行证高阶奖励资格", subscription: "订阅资格" }

function formatTime(value) { return new Date(value * 1000).toLocaleString("zh-CN", { hour12: false }) }

export function rewardAssetText(reward) {
    const label = assetLabels[reward.kind] ?? `奖励类型 ${reward.kind}`
    const identity = reward.type_id === null || reward.type_id === undefined ? "" : ` #${reward.type_id}`
    return `${label}${identity} x${reward.number}`
}

// //// 按存档和记录类型管理分页与展示 [@x380kkm 2026-09-22] ////
export function createRewardHistory({ requestApi, runAction, document = globalThis.document }) {
    const controls = Object.fromEntries(["save", "view", "state", "list", "refresh", "more"].map((key) => [key, document.querySelector(`#reward-history-${key}`)]))
    let entries = []
    let next = null
    let generation = 0

    function element(tag, className, text) {
        const value = document.createElement(tag)
        value.className = className
        value.textContent = text
        return value
    }

    function reset() {
        generation += 1
        entries = []
        next = null
        controls.list.replaceChildren()
        controls.more.hidden = true
        controls.state.textContent = "选择存档后查看奖励记录."
    }

    function receivedCard(entry) {
        const card = element("article", "item-card", "")
        card.append(element("strong", "", rewardAssetText(entry)))
        card.append(element("p", "meta", `${sourceLabels[entry.source] ?? sourceLabels.reward} / 已到账 / ${formatTime(entry.created_at)}`))
        if (entry.subject) card.append(element("p", "", entry.subject))
        if (entry.description) card.append(element("p", "mail-body", entry.description))
        if (entry.reason_id) card.append(element("p", "meta", `来源编号 ${entry.reason_id}`))
        return card
    }

    function deliveryCard(entry) {
        const card = element("article", "item-card", "")
        card.append(element("strong", "", entry.title))
        card.append(element("p", "meta", `${deliveryLabels[entry.status] ?? "投递状态未知"} / 邮件 #${entry.id}`))
        card.append(element("p", "meta", `${entry.sender} / 投递 ${formatTime(entry.created_at)}`))
        card.append(element("p", "mail-body", entry.body))
        const rewards = element("ul", "", "")
        for (const reward of entry.rewards) rewards.append(element("li", "", rewardAssetText(reward)))
        card.append(rewards)
        if (entry.received_at !== null) card.append(element("p", "meta", `领取 ${formatTime(entry.received_at)}`))
        if (entry.expires_at !== null) card.append(element("p", "meta", `到期 ${formatTime(entry.expires_at)}`))
        return card
    }

    function productCard(entry) {
        const card = element("article", "item-card", "")
        card.append(element("strong", "", entry.product_id))
        card.append(element("p", "meta", `商品内容已发放 / ${formatTime(entry.created_at)}`))
        for (const kind of entry.entitlement_kinds) {
            card.append(element("p", "", entitlementLabels[kind] ?? kind))
        }
        card.append(element("p", "meta", entry.confirmed_at === null
            ? "完成界面待确认, 商品内容已发放."
            : `完成界面已确认 / ${formatTime(entry.confirmed_at)}`))
        return card
    }

    const views = {
        received: { route: "reward-history", renderEntry: receivedCard, label: "到账", empty: "此存档尚无到账记录." },
        products: { route: "product-history", renderEntry: productCard, label: "商品领取", empty: "此存档尚无商品领取记录." },
        deliveries: { route: "reward-deliveries", renderEntry: deliveryCard, label: "投递", empty: "此存档尚无邮件投递记录.", total: (payload) => `, 共 ${payload.total} 封邮件` },
    }

    function render(view) {
        controls.list.replaceChildren()
        for (const entry of entries) controls.list.append(view.renderEntry(entry))
        if (!entries.length) controls.list.append(element("p", "empty-state", view.empty))
        controls.more.hidden = next === null
    }

    async function load(append = false) {
        if (!controls.save.value) { reset(); return }
        if (!append) reset()
        const requestGeneration = ++generation
        const view = views[controls.view.value] ?? views.received
        const query = append && next !== null ? `?before_id=${next}` : ""
        controls.state.textContent = "正在读取奖励记录."
        try {
            const payload = await requestApi(`/v1/local-saves/${controls.save.value}/${view.route}${query}`)
            if (requestGeneration !== generation) return
            entries = append ? [...entries, ...payload.entries] : payload.entries
            next = payload.next_before_id
            render(view)
            controls.state.textContent = `${payload.slot_name}, 显示 ${entries.length} 条${view.label}记录${view.total?.(payload) ?? ""}.`
        } catch (error) {
            if (requestGeneration !== generation) return
            controls.state.textContent = "奖励记录读取失败, 请重试."
            throw error
        }
    }

    controls.save.addEventListener("change", () => runAction(controls.refresh, () => load()))
    controls.view.addEventListener("change", () => runAction(controls.refresh, () => load()))
    controls.refresh.addEventListener("click", () => runAction(controls.refresh, () => load()))
    controls.more.addEventListener("click", () => runAction(controls.more, () => load(true)))
    reset()
    return {
        setSaves(slots, activeSlotId) {
            const selected = controls.save.value
            controls.save.replaceChildren(element("option", "", "选择存档"))
            controls.save.options[0].value = ""
            for (const slot of slots) {
                const option = element("option", "", `${slot.name} (#${slot.id})`)
                option.value = String(slot.id)
                controls.save.append(option)
            }
            const preferred = slots.find((slot) => String(slot.id) === selected)
                ?? slots.find((slot) => slot.id === activeSlotId) ?? slots[0]
            controls.save.value = preferred ? String(preferred.id) : ""
            return runAction(controls.refresh, () => load())
        },
    }
}
// //// /按存档和记录类型管理分页与展示 ////
