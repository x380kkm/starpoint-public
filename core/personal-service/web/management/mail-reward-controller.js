// audience: external
// # personal-service-mail-reward-controller
//
// 此模块从个人服务读取邮件奖励目录, 并把用户选择转换为现有邮件奖励结构.

// //// 管理邮件奖励目录, 收藏, 选择和快捷发放 [@x380kkm 2026-08-20] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName)
    if (className) node.className = className
    if (text !== undefined) node.textContent = text
    return node
}

function normalizedSearchText(value) {
    return String(value ?? "").trim().toLocaleLowerCase("zh-CN")
}

function safeImageUrl(value) {
    if (!value) return null
    try {
        const url = new URL(value, location.origin)
        return url.origin === location.origin && url.pathname.startsWith("/manage/assets/")
            ? url.href
            : null
    } catch {
        return null
    }
}

const FALLBACK_REWARD_IMAGE_URL = "/manage/assets/item-placeholder.svg"

function appendOptionalImage(container, item) {
    const catalogImageUrl = safeImageUrl(item.image_url)
    const imageUrl = catalogImageUrl ?? FALLBACK_REWARD_IMAGE_URL
    const frame = createElement("figure", "mail-reward-art")
    const image = createElement("img")
    image.alt = ""
    image.loading = "lazy"
    image.dataset.rewardImageSource = catalogImageUrl === null ? "fallback" : "catalog"
    image.dataset.rewardImageState = "loading"
    image.addEventListener("load", () => {
        image.dataset.rewardImageState = "loaded"
    })
    image.addEventListener("error", () => {
        if (image.getAttribute("src") === FALLBACK_REWARD_IMAGE_URL) {
            image.dataset.rewardImageState = "failed"
            image.hidden = true
            return
        }
        image.dataset.rewardImageSource = "fallback"
        image.dataset.rewardImageState = "loading"
        image.src = FALLBACK_REWARD_IMAGE_URL
    })
    image.src = imageUrl
    frame.append(image)
    container.append(frame)
}

function mergeRewardValues(target, source, multiplier) {
    for (const [key, value] of Object.entries(source)) {
        if (typeof value === "number") {
            target[key] = (target[key] ?? 0) + value * multiplier
            continue
        }
        if (value && typeof value === "object" && !Array.isArray(value)) {
            const nested = target[key] ?? {}
            target[key] = nested
            mergeRewardValues(nested, value, multiplier)
        }
    }
}

function requireValidRewardAmount(amount) {
    if (!Number.isInteger(amount) || amount <= 0) {
        throw new Error("数量必须是正整数.")
    }
    return amount
}

export function configureRewardAmountInput(input, itemName, initialAmount) {
    input.type = "number"
    input.min = "1"
    input.max = "999999999"
    input.step = "1"
    input.value = String(requireValidRewardAmount(initialAmount))
    input.setAttribute("aria-label", `${itemName} 数量`)
}

export function buildMailRewards(selection) {
    const rewards = {}
    for (const { item, amount } of selection) {
        mergeRewardValues(rewards, item.rewards, requireValidRewardAmount(amount))
    }
    return rewards
}

function hasRewards(rewards) {
    return Object.keys(rewards).length > 0
}

export function createMailRewardController({ elements, requestApi, runAction, sendMail }) {
    const state = {
        catalog: { items: [], kinds: [], presets: [] },
        selection: new Map(),
    }

    function renderKinds() {
        const selected = elements.mailRewardKind.value || "all"
        elements.mailRewardKind.replaceChildren()
        const all = createElement("option", "", "全部")
        all.value = "all"
        all.selected = selected === "all"
        elements.mailRewardKind.append(all)
        for (const kind of state.catalog.kinds) {
            const option = createElement("option", "", kind.name)
            option.value = kind.key
            option.selected = kind.key === selected
            elements.mailRewardKind.append(option)
        }
        if (selected !== "all" && !state.catalog.kinds.some((kind) => kind.key === selected)) {
            elements.mailRewardKind.value = "all"
        }
    }

    function renderSelection() {
        elements.mailSelectionList.replaceChildren()
        if (state.selection.size === 0) {
            elements.mailSelectionList.append(createElement("p", "empty-state", "从上方目录添加奖励."))
        }
        for (const [key, entry] of state.selection) {
            const row = createElement("article", "mail-selection-item")
            const copy = createElement("div")
            copy.append(
                createElement("strong", "", entry.item.name),
                createElement("span", "meta", entry.item.key),
            )
            const amount = createElement("input", "mail-selection-amount")
            configureRewardAmountInput(amount, entry.item.name, entry.amount)
            amount.addEventListener("change", () => {
                const nextAmount = Number(amount.value)
                try {
                    requireValidRewardAmount(nextAmount)
                    entry.amount = nextAmount
                    amount.setCustomValidity("")
                    syncPayload()
                } catch (error) {
                    amount.value = String(entry.amount)
                    amount.setCustomValidity(error.message ?? String(error))
                    amount.reportValidity()
                }
            })
            const remove = createElement("button", "button danger", "移除")
            remove.type = "button"
            remove.addEventListener("click", () => {
                state.selection.delete(key)
                renderSelection()
            })
            row.append(copy, amount, remove)
            elements.mailSelectionList.append(row)
        }
        elements.mailSelectionCount.textContent = `${state.selection.size} 项`
        syncPayload()
    }

    function syncPayload() {
        elements.mailRewardsPayload.value = JSON.stringify(buildMailRewards(state.selection.values()))
    }

    function addSelection(item, amount) {
        requireValidRewardAmount(amount)
        const current = state.selection.get(item.key)
        state.selection.set(item.key, {
            item,
            amount: (current?.amount ?? 0) + amount,
        })
        renderSelection()
    }

    function renderCatalog() {
        const query = normalizedSearchText(elements.mailRewardSearch.value)
        const kind = elements.mailRewardKind.value
        const favoritesOnly = elements.mailRewardFavorites.checked
        const visibleItems = state.catalog.items.filter((item) => {
            if (kind !== "all" && item.kind !== kind) return false
            if (favoritesOnly && !item.favorite) return false
            const haystack = normalizedSearchText(
                `${item.name} ${item.key} ${item.resource_id ?? ""} ${item.string_id ?? ""} `
                + `${item.thumbnail_id ?? ""} ${item.description ?? ""} ${item.kind ?? ""} `
                + `${item.kind_name ?? ""} ${item.effect_kind ?? ""} ${item.category ?? ""} ${item.group ?? ""}`,
            )
            return !query || haystack.includes(query)
        })
        elements.mailRewardList.replaceChildren()
        if (visibleItems.length === 0) {
            elements.mailRewardList.append(createElement("p", "empty-state", "没有符合条件的奖励."))
            return
        }
        for (const item of visibleItems) {
            const card = createElement("article", "mail-reward-card")
            appendOptionalImage(card, item)
            const content = createElement("div", "mail-reward-card-content")
            const heading = createElement("div", "mail-reward-card-heading")
            const title = createElement("div")
            title.append(
                createElement("h4", "card-title", item.name),
                createElement(
                    "span",
                    "meta",
                    item.resource_id
                        ? `ID ${item.resource_id} / ${item.kind_name ?? item.kind} / ${item.key}`
                        : item.key,
                ),
            )
            const favorite = createElement("button", `mail-favorite${item.favorite ? " active" : ""}`, item.favorite ? "★" : "☆")
            favorite.type = "button"
            favorite.setAttribute("aria-label", item.favorite ? `取消收藏 ${item.name}` : `收藏 ${item.name}`)
            favorite.addEventListener("click", () => runAction(favorite, async () => {
                const result = await requestApi(`/v1/mail-rewards/catalog/${encodeURIComponent(item.key)}/favorite`, {
                    method: "PUT",
                    body: { favorite: !item.favorite },
                })
                item.favorite = result.favorite
                renderCatalog()
                return result.favorite ? `已收藏 ${item.name}.` : `已取消收藏 ${item.name}.`
            }))
            heading.append(title, favorite)
            content.append(heading)
            if (item.description) content.append(createElement("p", "meta mail-reward-description", item.description))
            const actions = createElement("div", "mail-reward-actions")
            const amount = createElement("input")
            configureRewardAmountInput(amount, item.name, item.default_amount ?? 1)
            const add = createElement("button", "button ghost", "添加")
            add.type = "button"
            add.addEventListener("click", () => addSelection(item, Number(amount.value)))
            actions.append(amount, add)
            content.append(actions)
            card.append(content)
            elements.mailRewardList.append(card)
        }
    }

    function renderPresets() {
        elements.mailPresetList.replaceChildren()
        if (state.catalog.presets.length === 0) {
            elements.mailPresetList.append(createElement("p", "empty-state", "当前资源版本没有可用的快捷补给."))
            return
        }
        for (const preset of state.catalog.presets) {
            const card = createElement("article", "mail-preset-card")
            appendOptionalImage(card, preset)
            const content = createElement("div", "mail-preset-content")
            content.append(createElement("h4", "card-title", preset.name))
            content.append(createElement("p", "meta", preset.description ?? ""))
            const grant = createElement("button", "button primary", "立即发放")
            grant.type = "button"
            grant.addEventListener("click", () => runAction(grant, async () => {
                if (!confirm(`向当前存档发放“${preset.name}”?`)) return undefined
                await sendMail({
                    title: preset.name,
                    body: preset.description || "由当前设备的个人服务发放.",
                    sender: "Starpoint",
                    rewards: preset.rewards,
                })
                return `${preset.name} 已发送到游戏邮箱.`
            }))
            content.append(grant)
            card.append(content)
            elements.mailPresetList.append(card)
        }
    }

    async function load() {
        elements.mailCatalogState.dataset.loadState = "loading"
        elements.mailCatalogState.dataset.itemCount = "0"
        try {
            state.catalog = await requestApi("/v1/mail-rewards/catalog")
        } catch (error) {
            state.catalog = { items: [], kinds: [], presets: [] }
            renderKinds()
            renderPresets()
            renderCatalog()
            elements.mailCatalogState.textContent = "奖励目录暂时不可用"
            elements.mailCatalogState.dataset.loadState = "failed"
            return
        }
        renderKinds()
        renderPresets()
        renderCatalog()
        elements.mailCatalogState.textContent = `${state.catalog.items.length} 项奖励, ${state.catalog.presets.length} 个快捷补给`
        elements.mailCatalogState.dataset.itemCount = String(state.catalog.items.length)
        elements.mailCatalogState.dataset.loadState = state.catalog.items.length > 0 ? "loaded" : "failed"
    }

    elements.mailRewardSearch.addEventListener("input", renderCatalog)
    elements.mailRewardKind.addEventListener("change", renderCatalog)
    elements.mailRewardFavorites.addEventListener("change", renderCatalog)
    elements.mailSelectionClear.addEventListener("click", () => {
        state.selection.clear()
        renderSelection()
    })
    renderSelection()

    return {
        clearSelection() {
            state.selection.clear()
            renderSelection()
        },
        hasSelection() {
            return hasRewards(buildMailRewards(state.selection.values()))
        },
        load,
    }
}
// //// /管理邮件奖励目录, 收藏, 选择和快捷发放 ////
