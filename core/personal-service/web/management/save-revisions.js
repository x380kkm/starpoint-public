// audience: internal
// # management-save-revisions
// 版本历史与普通快照分别展示. 恢复携带读取时的当前 ETag, 存档变化由服务拒绝覆盖.

import { actionButton, node } from "./save-controls.js"

// //// 展示版本历史并恢复选定版本 [@x380kkm 2026-09-26] ////
export async function renderSaveRevisions(slot, container, actions) {
    const document = container.ownerDocument
    const path = `/v1/local-saves/${slot.id}/revisions`
    const state = await actions.requestApi(path)
    const current = state.revisions.find((revision) => revision.id === state.current_revision_id)
    const section = node(document, "section", "automation-card")
    section.append(node(document, "h4", "card-title", "版本历史"))
    section.append(node(document, "p", "meta", "包含同步覆盖前的安全版本. 普通快照在查看快照中单独展示."))
    const list = node(document, "ul", "snapshot-list")
    const sources = {
        "Before transfer upload": "同步覆盖前", "Transfer upload": "同步导入",
        "Before restore": "恢复前", "Snapshot restore": "快照恢复",
        "Revision restore": "版本恢复", "Initial save": "初始存档",
        "Current state": "当前状态",
    }
    let restoring = false
    for (const revision of state.revisions) {
        const item = node(document, "li")
        const description = node(document, "span")
        description.append(node(document, "strong", "", sources[revision.label] || revision.label))
        description.append(node(document, "span", "meta", `${revision.created_at} · ${revision.id}`))
        item.append(description)
        if (revision.id === state.current_revision_id) item.append(node(document, "span", "badge", "当前版本"))
        else {
            const button = actionButton(document, "恢复此版本", async () => {
                if (restoring) return undefined
                const confirm = actions.confirm || window.confirm.bind(window)
                if (!confirm(`将 ${slot.name} 恢复到 ${revision.created_at} 的版本? 当前状态会保留为安全版本.`)) return undefined
                if (!current) throw new Error("当前版本已变化, 请重新读取版本历史.")
                restoring = true
                try {
                    await actions.requestApi(`${path}/${encodeURIComponent(revision.id)}/restore`, {
                        method: "POST", headers: { "If-Match": `"${current.etag}"` },
                    })
                    await actions.refreshManagementState()
                    return "存档版本已恢复, 覆盖前状态保留在版本历史中."
                } finally { restoring = false }
            }, actions.runAction, "danger")
            button.disabled = !current
            item.append(button)
        }
        list.append(item)
    }
    if (!state.revisions.length) list.append(node(document, "li", "", "尚无版本历史."))
    section.append(list)
    container.replaceChildren(section)
}
// //// /展示版本历史并恢复选定版本 ////
