// audience: external
// # management-password-backup
// 密码备份包含完整存档. 旧加密存档可与原设备恢复包一起导入.

// //// 在存档卡片内输入并确认备份密码 [@x380kkm 2026-09-26] ////
export function requestBackupPassword(container) {
    return new Promise((resolve) => {
        const form = document.createElement("form")
        form.className = "stack-form save-password-panel"
        const fields = ["备份密码, 8 到 128 个字符", "再次输入密码"].map((text) => {
            const label = document.createElement("label")
            const title = document.createElement("span")
            title.textContent = text
            const input = document.createElement("input")
            input.type = "password"
            input.autocomplete = "new-password"
            input.required = true
            label.append(title, input)
            form.append(label)
            return input
        })
        const message = document.createElement("p")
        message.className = "meta"
        message.textContent = "在另一台设备导入此文件时使用同一密码."
        const submit = document.createElement("button")
        submit.className = "button primary"
        submit.type = "submit"
        submit.textContent = "加密并导出"
        const cancel = document.createElement("button")
        cancel.className = "button secondary"
        cancel.type = "button"
        cancel.textContent = "取消"
        const finish = (value) => {
            fields.forEach((field) => { field.value = "" })
            form.remove()
            resolve(value)
        }
        cancel.addEventListener("click", () => finish(undefined))
        form.addEventListener("submit", (event) => {
            event.preventDefault()
            const password = fields[0].value
            const length = Array.from(password).length
            if (length < 8 || length > 128 || password !== fields[1].value) {
                message.textContent = "请输入 8 到 128 个字符的密码, 并确保两次输入一致."
                return
            }
            finish(password)
        })
        form.append(message, submit, cancel)
        container.append(form)
        fields[0].focus()
    })
}

// //// 按存档封装选择导入与密码恢复接口 [@x380kkm 2026-09-26] ////
export async function importSaveFile(form, requestApi) {
    const file = form.get("file")
    if (!(file instanceof File) || file.size === 0) throw new Error("请选择存档文件.")
    const name = String(form.get("name") ?? "").trim()
    const payload = JSON.parse(await file.text())
    const password = String(form.get("password") ?? "")
    if (payload.format === "starpoint-password-save") {
        return requestApi("/v1/local-saves/import-password", { method: "POST", body: { name, password, package: payload } })
    }
    if (payload.format === "starpoint-encrypted-save") {
        const recoveryFile = form.get("recovery")
        if (recoveryFile instanceof File && recoveryFile.size > 0) {
            const recoveryData = JSON.parse(await recoveryFile.text())
            const recovery = recoveryData?.package ?? recoveryData
            return requestApi("/v1/local-saves/import-encrypted-recovery", { method: "POST", body: { name, password, envelope: payload, recovery } })
        }
        return requestApi("/v1/local-saves/import-encrypted", { method: "POST", body: { name, envelope: payload } })
    }
    return requestApi("/v1/local-saves/import", { method: "POST", body: { name, data: payload } })
}
