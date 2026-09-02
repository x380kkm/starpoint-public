// audience: external
// # personal-service-ai-team-controller
//
// 此模块从当前本地存档读取有效编队, 并保存两个可传输到联机服务器的编队快照.

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
    }

    function selectedSlotId() {
        const slotId = Number(elements.aiTeamSlot.value)
        return Number.isInteger(slotId) && slotId > 0 ? slotId : null
    }

    function setStatus(message) {
        elements.aiTeamState.textContent = message
    }

    function renderCandidateList() {
        elements.aiTeamCandidates.replaceChildren()
        if (state.candidates.length === 0) {
            elements.aiTeamCandidates.append(createElement("p", "empty-state", "当前存档没有可用编队."))
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
        const messages = {
            ready: `已保存 AI A 和 AI B 的两个编队模板, 共发现 ${state.candidates.length} 个有效编队.`,
            default_template_required: "当前存档少于两个有效编队, 需要先在游戏中保存更多编队.",
            manual_selection_required: "自动选择已关闭, 请为两个 AI 重新选择编队.",
            selection_unavailable: "当前存档无法建立两个 AI 编队快照.",
        }
        setStatus(messages[response.selection_status] ?? "AI 编队状态已载入.")
    }

    async function load() {
        const slotId = selectedSlotId()
        const generation = ++state.generation
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
        if (slotId === null) throw new Error("请选择本地存档槽.")
        const partyIds = requireDistinctPartyIds([elements.aiTeamA.value, elements.aiTeamB.value])
        const response = await requestApi(`/v1/local-saves/${slotId}/ai-teams`, {
            method: "PUT",
            body: { party_ids: partyIds },
        })
        renderSelection(response)
    }

    function setSlots(slots, preferredSlotId) {
        const currentSlotId = selectedSlotId()
        state.slots = Array.isArray(slots) ? slots : []
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
        return "两个 AI 编队模板已保存为当前存档快照."
    }))
    elements.aiTeamDefault.addEventListener("click", () => runAction(elements.aiTeamDefault, async () => {
        const defaults = state.candidates.slice(0, 2).map((candidate) => candidate.party_id)
        const partyIds = requireDistinctPartyIds(defaults)
        elements.aiTeamA.value = String(partyIds[0])
        elements.aiTeamB.value = String(partyIds[1])
        await saveSelected()
        return "已恢复当前存档的前两个有效编队."
    }))

    return { load, setSlots }
}
// //// /选择并保存两个不同的存档编队 ////
