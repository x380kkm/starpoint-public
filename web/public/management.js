// audience: external
// # management-page-client
// 此脚本使用同源管理 API 登录, 渲染权限范围内的数据并提交显式用户操作.

const output = document.querySelector("#output")
const notice = document.querySelector("#notice")
const identity = document.querySelector("#identity")
const loginButton = document.querySelector("#login")
const logoutButton = document.querySelector("#logout")
const refreshButton = document.querySelector("#refresh-all")
const savesPanel = document.querySelector("#saves-panel")
const outputPanel = document.querySelector("#output-panel")
const saves = document.querySelector("#saves")
const usersBody = document.querySelector("#users")
const accountsBody = document.querySelector("#accounts")
const backups = document.querySelector("#backups")
const mails = document.querySelector("#mails")
const accountPageSize = 25
let accountOffset = 0
let currentUser = null

// //// 发送带同源 cookie 且不缓存的 JSON 请求 [@x380kkm 2026-07-22] ////
const request = async (url, options = {}) => {
    const headers = { ...(options.headers || {}) }
    if (options.body !== undefined) headers["content-type"] = "application/json"
    const response = await fetch(url, {
        ...options,
        cache: "no-store",
        credentials: "same-origin",
        headers,
    })
    const text = await response.text()
    const data = text.length === 0 ? {} : JSON.parse(text)
    if (!response.ok) throw new Error(data.message || data.error || `HTTP ${response.status}`)
    return data
}

const show = (value) => {
    outputPanel.classList.remove("hidden")
    output.textContent = JSON.stringify(value, null, 2)
}

const run = async (action) => {
    notice.textContent = ""
    try {
        return await action()
    } catch (error) {
        notice.textContent = error.message
        show({ error: error.message })
        return null
    }
}
// //// /发送带同源 cookie 且不缓存的 JSON 请求 ////

// //// 根据登录身份显示玩家和管理员功能 [@x380kkm 2026-07-22] ////
const setIdentity = (user) => {
    currentUser = user
    const authenticated = user !== null
    document.querySelector("#username").disabled = authenticated
    document.querySelector("#password").disabled = authenticated
    loginButton.classList.toggle("hidden", authenticated)
    logoutButton.classList.toggle("hidden", !authenticated)
    refreshButton.classList.toggle("hidden", !authenticated)
    savesPanel.classList.toggle("hidden", !authenticated)
    outputPanel.classList.toggle("hidden", !authenticated)
    for (const section of document.querySelectorAll(".admin-only")) {
        section.classList.toggle("hidden", user?.role !== "admin")
    }
    identity.textContent = authenticated ? `${user.username} / ${user.role}` : "尚未登录."
}

const loadSession = async () => {
    const session = await request("/manage/api/auth/session")
    setIdentity(session.authenticated ? session.user : null)
    if (!session.configured) notice.textContent = "尚未配置管理员密码."
    return session
}

const login = async () => {
    const result = await request("/manage/api/auth/login", {
        method: "POST",
        body: JSON.stringify({
            username: document.querySelector("#username").value,
            password: document.querySelector("#password").value,
        }),
    })
    document.querySelector("#password").value = ""
    setIdentity(result.user)
    await refreshAll()
    return result
}

const logout = async () => {
    await request("/manage/api/auth/logout", { method: "POST" })
    setIdentity(null)
    saves.textContent = "尚未加载."
    return { loggedOut: true }
}
// //// /根据登录身份显示玩家和管理员功能 ////

// //// 导入和导出当前用户可访问的存档 [@x380kkm 2026-07-22] ////
const downloadSave = async (playerId) => {
    const response = await fetch(`/manage/api/saves/${playerId}`, {
        cache: "no-store",
        credentials: "same-origin",
    })
    if (!response.ok) {
        const error = await response.json()
        throw new Error(error.message || error.error || `HTTP ${response.status}`)
    }
    const blob = await response.blob()
    const url = URL.createObjectURL(blob)
    const link = document.createElement("a")
    link.href = url
    link.download = `starpoint-player-${playerId}.json`
    link.click()
    URL.revokeObjectURL(url)
}

const importSave = async (playerId, file) => {
    if (file === undefined) throw new Error("请选择 JSON 存档文件.")
    const payload = JSON.parse(await file.text())
    if (!confirm(`确认替换玩家 ${playerId} 的完整存档?`)) return null
    return request(`/manage/api/saves/${playerId}`, {
        method: "PUT",
        body: JSON.stringify(payload),
    })
}

const appendSaveActions = (container, playerId) => {
    const download = document.createElement("button")
    download.className = "secondary"
    download.textContent = "导出"
    download.onclick = () => run(() => downloadSave(playerId))
    const file = document.createElement("input")
    file.type = "file"
    file.accept = "application/json,.json"
    const upload = document.createElement("button")
    upload.textContent = "导入"
    upload.onclick = () => run(async () => show(await importSave(playerId, file.files?.[0])))
    container.append(download, file, upload)
}

const renderSaves = (players) => {
    saves.replaceChildren()
    for (const player of players) {
        const row = document.createElement("div")
        row.className = "row"
        const label = document.createElement("span")
        label.textContent = `${player.id} / ${player.name} / rank point ${player.rankPoint}`
        row.append(label)
        appendSaveActions(row, player.id)
        saves.append(row)
    }
    if (players.length === 0) saves.textContent = "当前账号没有绑定玩家."
}

const loadSaves = async () => {
    const value = await request("/manage/api/saves")
    renderSaves(value.players)
    return value
}
// //// /导入和导出当前用户可访问的存档 ////

// //// 渲染管理用户和玩家绑定 [@x380kkm 2026-07-22] ////
const renderUsers = (items) => {
    usersBody.replaceChildren()
    for (const user of items) {
        const row = document.createElement("tr")
        const values = [user.id, user.username, user.role, user.playerIds.join(", ") || "-", user.disabled ? "disabled" : "active"]
        for (const value of values) {
            const cell = document.createElement("td")
            cell.textContent = String(value)
            row.append(cell)
        }
        usersBody.append(row)
    }
    if (items.length === 0) usersBody.innerHTML = '<tr><td colspan="5" class="muted">没有管理用户.</td></tr>'
}

const loadUsers = async () => {
    const value = await request("/manage/api/users")
    renderUsers(value.users)
    return value
}
// //// /渲染管理用户和玩家绑定 ////

// //// 渲染游戏账号并提供显式会话和存档操作 [@x380kkm 2026-07-22] ////
const renderAccounts = (page) => {
    accountsBody.replaceChildren()
    for (const account of page.accounts) {
        const row = document.createElement("tr")
        const values = [
            account.id,
            `${account.appId} (${account.status})`,
            account.playerIds.join(", ") || "-",
            `ZAT ${account.sessionCounts.zat} / ZRT ${account.sessionCounts.zrt} / Viewer ${account.sessionCounts.viewer}`,
            account.lastLoginTime,
        ]
        for (const value of values) {
            const cell = document.createElement("td")
            cell.textContent = String(value)
            row.append(cell)
        }
        const actions = document.createElement("td")
        actions.className = "actions"
        const revoke = document.createElement("button")
        revoke.className = "danger"
        revoke.textContent = "撤销会话"
        revoke.onclick = () => run(async () => {
            if (!confirm(`确认撤销账号 ${account.id} 的全部会话?`)) return
            show(await request(`/manage/api/accounts/${account.id}/sessions`, { method: "DELETE" }))
            await loadAccounts()
        })
        const rotate = document.createElement("button")
        rotate.className = "secondary"
        rotate.textContent = "轮换 Viewer ID"
        rotate.onclick = () => run(async () => {
            if (!confirm(`确认轮换账号 ${account.id} 的 Viewer ID?`)) return
            show(await request(`/manage/api/accounts/${account.id}/viewer-id`, { method: "POST" }))
            await loadAccounts()
        })
        actions.append(revoke, rotate)
        for (const playerId of account.playerIds) {
            appendSaveActions(actions, playerId)
        }
        row.append(actions)
        accountsBody.append(row)
    }
    if (page.accounts.length === 0) accountsBody.innerHTML = '<tr><td colspan="6" class="muted">没有游戏账号.</td></tr>'
    document.querySelector("#previous-accounts").disabled = accountOffset === 0
    document.querySelector("#next-accounts").disabled = accountOffset + page.accounts.length >= page.total
}

const loadAccounts = async () => {
    const page = await request(`/manage/api/accounts?limit=${accountPageSize}&offset=${accountOffset}`)
    renderAccounts(page)
    return page
}
// //// /渲染游戏账号并提供显式会话和存档操作 ////

// //// 渲染备份并同步服务器状态 [@x380kkm 2026-07-22] ////
const renderBackups = (items) => {
    backups.replaceChildren()
    for (const backup of items) {
        const row = document.createElement("div")
        row.className = "row"
        const label = document.createElement("span")
        label.textContent = `${backup.id} / schema ${backup.schemaVersion} / ${backup.createdAt} / ${backup.files.length} files`
        const restore = document.createElement("button")
        restore.className = "danger"
        restore.textContent = backup.schemaVersion === 2 ? "暂存恢复" : "旧格式不可恢复"
        restore.disabled = backup.schemaVersion !== 2
        restore.onclick = () => run(async () => {
            if (!confirm(`确认校验并暂存备份 ${backup.id}? 服务重启前不会覆盖数据库.`)) return
            show(await request(`/manage/api/backups/${backup.id}/restore`, { method: "POST" }))
        })
        row.append(label, restore)
        backups.append(row)
    }
    if (items.length === 0) backups.textContent = "没有备份."
}

const loadBackups = async () => {
    const value = await request("/manage/api/backups")
    renderBackups(value.backups)
    return value
}

// //// 渲染并发放管理员邮件奖励 [@x380kkm 2026-07-24] ////
const renderMails = (value) => {
    mails.replaceChildren()
    for (const mail of value.mails) {
        const row = document.createElement("div")
        row.className = "row"
        const label = document.createElement("span")
        label.textContent = `${mail.id} / ${mail.title} / ${mail.createdAt}`
        row.append(label)
        mails.append(row)
    }
    if (value.mails.length === 0) mails.textContent = "没有未领取邮件."
}

const loadMails = async () => {
    const playerId = Number(document.querySelector("#mail-player-id").value)
    if (!Number.isSafeInteger(playerId) || playerId <= 0) throw new Error("请输入玩家 ID.")
    const value = await request(`/manage/api/mails/${playerId}`)
    renderMails(value)
    return value
}
// //// /渲染并发放管理员邮件奖励 ////

const loadStatus = async () => {
    const value = await request("/manage/api/status")
    const serverDate = new Date(value.serverDate)
    const localServerDate = new Date(serverDate.getTime() - serverDate.getTimezoneOffset() * 60_000)
    document.querySelector("#time").value = localServerDate.toISOString().slice(0, 16)
    document.querySelector("#rate").value = String(value.config.virtualTime.rate)
    document.querySelector("#npc").value = JSON.stringify({ ...value.config.npcFill, mates: value.config.npcMates }, null, 2)
    return value
}

const refreshAll = async () => {
    const playerData = await loadSaves()
    if (currentUser?.role !== "admin") {
        show({ saves: playerData })
        return
    }
    const [status, accounts, backupData, userData] = await Promise.all([loadStatus(), loadAccounts(), loadBackups(), loadUsers()])
    show({ status, accounts, backups: backupData, users: userData, saves: playerData })
}
// //// /渲染备份并同步服务器状态 ////

// //// 绑定页面按钮到管理 API [@x380kkm 2026-07-22] ////
loginButton.onclick = () => run(login)
logoutButton.onclick = () => run(logout)
refreshButton.onclick = () => run(refreshAll)
document.querySelector("#create-user").onclick = () => run(async () => {
    const user = await request("/manage/api/users", {
        method: "POST",
        body: JSON.stringify({
            username: document.querySelector("#new-username").value,
            password: document.querySelector("#new-password").value,
            role: document.querySelector("#new-role").value,
        }),
    })
    document.querySelector("#new-password").value = ""
    show(user)
    await loadUsers()
})
document.querySelector("#bind-player").onclick = () => run(async () => {
    const userId = Number(document.querySelector("#bind-user-id").value)
    const playerId = Number(document.querySelector("#bind-player-id").value)
    show(await request(`/manage/api/users/${userId}/players/${playerId}`, { method: "PUT" }))
    await loadUsers()
})
document.querySelector("#refresh-backups").onclick = () => run(loadBackups)
document.querySelector("#create-backup").onclick = () => run(async () => {
    show(await request("/manage/api/backups", { method: "POST" }))
    await loadBackups()
})
document.querySelector("#send-mail").onclick = () => run(async () => {
    const playerId = Number(document.querySelector("#mail-player-id").value)
    const rewards = JSON.parse(document.querySelector("#mail-rewards").value)
    const result = await request("/manage/api/mails", {
        method: "POST",
        body: JSON.stringify({
            playerId,
            title: document.querySelector("#mail-title").value,
            body: document.querySelector("#mail-body").value,
            sender: document.querySelector("#mail-sender").value,
            rewards,
        }),
    })
    show(result)
    await loadMails()
})
document.querySelector("#load-mails").onclick = () => run(async () => show(await loadMails()))
document.querySelector("#set-time").onclick = () => run(async () => {
    const value = document.querySelector("#time").value
    if (value.length === 0) throw new Error("请选择起始时间.")
    show(await request("/manage/api/time", {
        method: "PUT",
        body: JSON.stringify({ enabled: true, iso: new Date(value).toISOString(), rate: Number(document.querySelector("#rate").value) }),
    }))
})
document.querySelector("#reset-time").onclick = () => run(async () => {
    show(await request("/manage/api/time", { method: "PUT", body: JSON.stringify({ enabled: false }) }))
})
document.querySelector("#save-npc").onclick = () => run(async () => {
    show(await request("/manage/api/npc", { method: "PUT", body: document.querySelector("#npc").value }))
})
document.querySelector("#previous-accounts").onclick = () => run(async () => {
    accountOffset = Math.max(0, accountOffset - accountPageSize)
    show(await loadAccounts())
})
document.querySelector("#next-accounts").onclick = () => run(async () => {
    accountOffset += accountPageSize
    show(await loadAccounts())
})

void run(async () => {
    const session = await loadSession()
    if (session.authenticated) await refreshAll()
})
// //// /绑定页面按钮到管理 API ////
