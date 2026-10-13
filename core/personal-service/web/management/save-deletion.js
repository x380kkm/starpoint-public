// audience: external
// # management-save-deletion
//
// 删除确认显示存档身份和本地备份范围. 关闭对话框保留存档.

// //// 等待用户确认指定存档的删除 [@x380kkm 2026-09-20] ////
export async function confirmSaveDeletion(slot, opener) {
    const dialog = document.querySelector("#save-delete-dialog")
    if (!dialog || typeof dialog.showModal !== "function") {
        return window.confirm(
            `删除存档 "${slot.name}" (槽位 ${slot.id})?\n\n此存档的进度, 本地快照和 AI 编队将永久删除. 已导出的文件和远程副本会保留.`,
        )
    }
    if (dialog.open) return false
    dialog.querySelector("#save-delete-name").textContent = slot.name
    dialog.querySelector("#save-delete-detail").textContent = `槽位 ${slot.id} · ${slot.snapshot_count} 个本地快照`
    dialog.returnValue = "cancel"
    return new Promise((resolve) => {
        dialog.addEventListener("close", () => {
            resolve(dialog.returnValue === "delete")
            requestAnimationFrame(() => {
                if (opener.isConnected && !opener.disabled) opener.focus({ preventScroll: true })
            })
        }, { once: true })
        dialog.showModal()
    })
}
