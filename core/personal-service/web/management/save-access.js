// audience: internal
// # management-save-access
// 槽授权仅在签发响应中显示明文, 列表使用授权元数据, 撤销后清除页面保存的对应授权.

import { actionButton, field, node, submitForm } from "./save-controls.js"

const PERMISSIONS = [["download", "仅下载"], ["upload", "仅上传"], ["both", "双向同步"]]

// //// 管理单个存档槽的传输授权 [@x380kkm 2026-09-26] ////
export function renderSlotAccess(slot, container, actions) {
    const document = container.ownerDocument
    const section = node(document, "details", "form-drawer")
    section.append(node(document, "summary", "", "槽位传输授权"))
    const identity = node(document, "p", "meta", `本机实例, 槽位 ${slot.id}. 展开后读取完整实例标识.`)
    section.append(identity)
    const list = node(document, "div", "card-list")
    const issuedContainer = node(document, "div", "stack-form")
    const form = node(document, "form", "stack-form")
    const fields = node(document, "fieldset", "stack-form")
    const device = field(document, fields, "device_name", "授权名称", { maxLength: 128 })
    const permission = field(document, fields, "permission", "允许操作", { choices: PERMISSIONS, value: "download" })
    const expires = field(document, fields, "expires_at", "到期时间 (UTC, 留空为长期)", { type: "datetime-local" })
    fields.append(node(document, "p", "meta", "跨实例槽位绑定使用双向同步授权. 授权资料只交给你信任的目标设备."))
    form.append(fields)
    let issued = null
    let generation = 0
    let loaded = false
    const path = `/v1/local-saves/${slot.id}/transfer-tokens/slot`

    async function load() {
        const request = ++generation
        const response = await actions.requestApi(path)
        if (request !== generation) return
        identity.textContent = `实例类型 local, 实例 ID ${response.instanceId}, 槽位 ID ${slot.id}.`
        list.replaceChildren()
        for (const token of response.tokens) {
            const card = node(document, "article", "item-card")
            const permissionLabel = PERMISSIONS.find(([value]) => value === token.permission)
            const expired = token.expiresAt && Date.parse(token.expiresAt) <= Date.now()
            const status = token.revokedAt ? "已撤销" : expired ? "已过期" : "有效"
            card.append(node(document, "strong", "", token.deviceName || token.id))
            card.append(node(document, "p", "meta", `${permissionLabel ? permissionLabel[1] : token.permission} · ${status} · 创建 ${token.createdAt}`))
            if (token.expiresAt) card.append(node(document, "p", "meta", `到期 ${token.expiresAt}`))
            if (!token.revokedAt) {
                card.append(actionButton(document, "撤销槽授权", async () => {
                    const confirm = actions.confirm || window.confirm.bind(window)
                    if (!confirm(`撤销 ${token.deviceName || token.id} 的槽授权? 使用此授权的同步需要重新授权.`)) return undefined
                    await actions.requestApi(`${path}/${encodeURIComponent(token.id)}`, { method: "DELETE" })
                    if (issued && issued.metadata.id === token.id) {
                        issued = null
                        issuedContainer.replaceChildren()
                    }
                    await load()
                    return "槽授权已撤销."
                }, actions.runAction, "danger"))
            }
            list.append(card)
        }
        if (!response.tokens.length) list.append(node(document, "p", "empty-state", "此槽位尚无传输授权."))
        loaded = true
    }

    submitForm(document, form, fields, "签发槽授权", async () => {
        const body = { permission: permission.value }
        if (device.value.trim()) body.deviceName = device.value.trim()
        if (expires.value) {
            const date = new Date(`${expires.value}Z`)
            if (!Number.isFinite(date.getTime()) || date.getTime() <= Date.now()) throw new Error("请输入将来的 UTC 到期时间.")
            body.expiresAt = date.toISOString()
        }
        issued = await actions.requestApi(path, { method: "POST", body })
        const label = node(document, "label", "stack-form")
        label.append(node(document, "span", "", "本次槽授权资料, 可粘贴到另一设备的创建槽位绑定表单"))
        const contents = node(document, "textarea")
        contents.readOnly = true
        contents.rows = 7
        contents.spellcheck = false
        contents.value = JSON.stringify({
            target_instance_kind: "local", target_instance_id: issued.instanceId,
            target_slot_id: slot.id, target_token: issued.token,
        }, null, 2)
        label.append(contents)
        issuedContainer.replaceChildren(label, actionButton(document, "选择授权资料", () => { contents.focus(); contents.select() }, actions.runAction))
        identity.textContent = `实例类型 local, 实例 ID ${issued.instanceId}, 槽位 ID ${slot.id}.`
        await load()
        return "槽授权已签发, 明文仅在当前页面显示."
    }, actions.runAction)
    section.addEventListener("toggle", () => {
        if (section.open && !loaded) return actions.runAction(null, load)
    })
    section.append(actionButton(document, "刷新槽授权", load, actions.runAction), list, form, issuedContainer)
    container.append(section)
}
// //// /管理单个存档槽的传输授权 ////
