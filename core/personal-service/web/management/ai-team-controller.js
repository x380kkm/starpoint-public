// audience: external
// # personal-service-ai-team-controller
//
// 此模块从所选本地存档读取有效编队, 并保存供本机建房和救援使用的两个编队快照.

// //// 选择并保存两个不同的存档编队 [@x380kkm 2026-08-20] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName)
    if (className) node.className = className
    if (text !== undefined) node.textContent = text
    return node
}

export function requireDistinctPartyIds(values) {
    const partyIds = values.map(Number)
    if (partyIds.length !== 2 || partyIds.some((partyId) => !Number.isInteger(partyId) || partyId <= 0)) {
        throw new Error("请为两个 AI 分别选择有效编队.")
    }
    if (partyIds[0] === partyIds[1]) throw new Error("两个 AI 必须使用不同编队.")
    return partyIds
}

export function describePartyCandidate(candidate) {
    const partyId = Number(candidate.party_id)
    const name = String(candidate.name ?? "").trim() || `编队 ${partyId}`
    const characterIds = Array.isArray(candidate.character_ids)
        ? candidate.character_ids.filter((value) => Number.isInteger(Number(value)) && Number(value) > 0)
        : []
    const characterText = characterIds.length > 0
        ? `角色 ${characterIds.join(" / ")}`
        : "没有可显示的角色"
    return { partyId, name, characterText }
}

export function createAiTeamController({ elements, requestApi, runAction }) {
    const state = {
        candidates: [],
        generation: 0,
        slots: [],
        sourceLoaded: false,
        currentSlotId: null,
    }

    function selectedSlotId() {
        const slotId = Number(elements.aiTeamSlot.value)
        return Number.isInteger(slotId) && slotId > 0 ? slotId : null
    }

    function setStatus(message) {
        elements.aiTeamState.textContent = message
    }

    function renderSource(source) {
        const slot = state.slots.find((slot) => Number(slot.id) === Number(source?.slot_id))
        elements.aiTeamSource.textContent = slot
            ? `实际来源: ${slot.name} (槽位 ${slot.id}). 新建房间和救援使用该存档的两个已保存编队.`
            : "实际来源: 跟随当前游戏存档. 新建房间和救援读取当前玩家的编队."
        elements.aiTeamFollow.disabled = !slot
    }

    function renderCandidateList() {
        elements.aiTeamCandidates.replaceChildren()
        if (state.candidates.length === 0) {
            elements.aiTeamCandidates.append(createElement("p", "empty-state", "所选存档没有可用编队."))
            return
        }
        for (const candidate of state.candidates) {
            const description = describePartyCandidate(candidate)
            const card = createElement("article", "ai-party-candidate")
            card.append(
                createElement("strong", "", description.name),
                createElement("span", "meta", `编队 ID ${description.partyId}`),
                createElement("span", "meta", description.characterText),
            )
            elements.aiTeamCandidates.append(card)
        }
    }

    function fillSelector(select, selectedPartyId) {
        select.replaceChildren()
        for (const candidate of state.candidates) {
            const description = describePartyCandidate(candidate)
            const option = createElement("option", "", `${description.name} · ${description.characterText}`)
            option.value = String(description.partyId)
            option.selected = description.partyId === selectedPartyId
            select.append(option)
        }
    }

    function keepSelectionsDistinct(changedSelect) {
        const otherSelect = changedSelect === elements.aiTeamA ? elements.aiTeamB : elements.aiTeamA
        for (const option of otherSelect.options) option.disabled = option.value === changedSelect.value
        if (otherSelect.value !== changedSelect.value) return
        const alternative = Array.from(otherSelect.options).find((option) => !option.disabled)
        if (alternative) otherSelect.value = alternative.value
    }

    function renderSelection(response) {
        renderSource(response.source)
        state.candidates = Array.isArray(response.candidates) ? response.candidates : []
        const selected = Array.isArray(response.selected_party_ids)
            ? response.selected_party_ids.map(Number)
            : []
        const defaults = state.candidates.slice(0, 2).map((candidate) => Number(candidate.party_id))
        fillSelector(elements.aiTeamA, selected[0] ?? defaults[0])
        fillSelector(elements.aiTeamB, selected[1] ?? defaults[1])
        if (elements.aiTeamA.value) keepSelectionsDistinct(elements.aiTeamA)
        if (elements.aiTeamB.value) keepSelectionsDistinct(elements.aiTeamB)
        renderCandidateList()
        const usable = state.candidates.length >= 2
        elements.aiTeamA.disabled = !usable
        elements.aiTeamB.disabled = !usable
        elements.aiTeamSave.disabled = !usable
        elements.aiTeamDefault.disabled = !usable
        const isActiveSource = response.source?.mode === "selected"
            && Number(response.source.slot_id) === Number(response.slot_id)
        const messages = {
            ready: isActiveSource
                ? `该存档的 AI A 和 AI B 已用于后续建房和救援, 共发现 ${state.candidates.length} 个有效编队.`
                : `所选存档已保存两个编队模板, 共发现 ${state.candidates.length} 个有效编队. 点击保存并使用可将它设为实际来源.`,
            default_template_required: "所选存档使用内置 AI 队伍. 保存两个有效编队后可选用自己的队伍.",
            manual_selection_required: "所选存档使用内置 AI 队伍. 可在这里选择并保存自己的两个编队.",
            selection_unavailable: "所选存档无法建立两个 AI 编队快照.",
        }
        setStatus(messages[response.selection_status] ?? "AI 编队状态已载入.")
    }

    async function load() {
        const generation = ++state.generation
        elements.aiTeamA.disabled = true
        elements.aiTeamB.disabled = true
        elements.aiTeamSave.disabled = true
        elements.aiTeamDefault.disabled = true
        const source = await requestApi("/v1/local-saves/ai-team-source")
        if (generation !== state.generation) return
        renderSource(source)
        if (!state.sourceLoaded) {
            const slot = state.slots.find((slot) => Number(slot.id) === Number(source.slot_id))
            if (slot) elements.aiTeamSlot.value = String(slot.id)
            state.sourceLoaded = true
        }
        const slotId = selectedSlotId()
        if (slotId === null) {
            state.candidates = []
            renderCandidateList()
            setStatus("当前没有可管理的本地存档槽.")
            return
        }
        setStatus("正在读取存档编队.")
        const response = await requestApi(`/v1/local-saves/${slotId}/ai-teams`)
        if (generation !== state.generation || slotId !== selectedSlotId()) return
        renderSelection(response)
    }

    async function saveSelected() {
        const slotId = selectedSlotId()
        const generation = state.generation
        if (slotId === null) throw new Error("请选择本地存档槽.")
        const partyIds = requireDistinctPartyIds([elements.aiTeamA.value, elements.aiTeamB.value])
        const response = await requestApi(`/v1/local-saves/${slotId}/ai-teams`, {
            method: "PUT",
            body: { party_ids: partyIds },
        })
        renderSource(response.source)
        if (generation === state.generation && slotId === selectedSlotId()) renderSelection(response)
    }

    function setSlots(slots, preferredSlotId) {
        const currentSlotId = selectedSlotId()
        state.slots = Array.isArray(slots) ? slots : []
        state.currentSlotId = Number(preferredSlotId)
        elements.aiTeamSlot.replaceChildren()
        for (const slot of state.slots) {
            const option = createElement("option", "", `${slot.name} (槽位 ${slot.id})`)
            option.value = String(slot.id)
            elements.aiTeamSlot.append(option)
        }
        const availableIds = new Set(state.slots.map((slot) => Number(slot.id)))
        const nextSlotId = availableIds.has(currentSlotId)
            ? currentSlotId
            : availableIds.has(Number(preferredSlotId))
                ? Number(preferredSlotId)
                : Number(state.slots[0]?.id)
        if (Number.isInteger(nextSlotId)) elements.aiTeamSlot.value = String(nextSlotId)
    }

    elements.aiTeamA.addEventListener("change", () => keepSelectionsDistinct(elements.aiTeamA))
    elements.aiTeamB.addEventListener("change", () => keepSelectionsDistinct(elements.aiTeamB))
    elements.aiTeamSlot.addEventListener("change", () => runAction(null, load))
    elements.aiTeamSave.addEventListener("click", () => runAction(elements.aiTeamSave, async () => {
        await saveSelected()
        return "两个 AI 编队已保存, 后续建房和救援使用该存档."
    }))
    elements.aiTeamDefault.addEventListener("click", () => runAction(elements.aiTeamDefault, async () => {
        const defaults = state.candidates.slice(0, 2).map((candidate) => candidate.party_id)
        const partyIds = requireDistinctPartyIds(defaults)
        elements.aiTeamA.value = String(partyIds[0])
        elements.aiTeamB.value = String(partyIds[1])
        await saveSelected()
        return "已保存并使用所选存档的前两个有效编队."
    }))
    elements.aiTeamFollow.addEventListener("click", () => runAction(elements.aiTeamFollow, async () => {
        renderSource(await requestApi("/v1/local-saves/ai-team-source", { method: "DELETE" }))
        if (state.slots.some((slot) => Number(slot.id) === state.currentSlotId)) {
            elements.aiTeamSlot.value = String(state.currentSlotId)
        }
        await load()
        return "AI 来源已恢复为当前游戏存档, 已有房间的队友保持原编队."
    }))

    return { load, setSlots }
}
// //// /选择并保存两个不同的存档编队 ////
