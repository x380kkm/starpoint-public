// audience: external
// # personal-service-activity-views
//
// 此模块把活动目录和图片候选元数据转换为安全 DOM 节点. 所有目录文本只写入 textContent.

// //// 呈现活动目录和图片 [@x380kkm 2026-08-19] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName)
    if (className) node.className = className
    if (text !== undefined) node.textContent = text
    return node
}

function createButton(label, action, runAction, style = "ghost") {
    const control = createElement("button", `button ${style}`, label)
    control.type = "button"
    control.addEventListener("click", () => runAction(control, action))
    return control
}

export function renderActivityCatalog(activities, elements, actions) {
    fillActivityKindFilter(activities, elements)
    const taggedActivities = filterActivitiesByTag(activities)
    renderActivityCalendar(activities, elements, actions, taggedActivities)
    const visibleActivities = filterActivitiesByDate(activities, taggedActivities)
    renderActivityQuickFilters(activities, elements, actions, visibleActivities.length)
    renderActivityTagFilters(activities, elements, actions)
    elements.activityCatalogList.replaceChildren()
    if (activities.error) {
        elements.activityCatalogState.textContent = "活动目录尚不可用, 旧版 Raid Boss 管理仍可使用."
        elements.activityCatalogList.append(createElement("p", "empty-state", activities.error.message))
        renderActivityDetail(activities, elements, actions)
        return
    }
    const catalogSummary = `当前结果 ${visibleActivities.length} 项, 目录匹配 ${activities.items.length} 项`
    const sourceSummary = activities.manifestState === "missing"
        ? "活动资源目录尚未提取"
        : [
            activities.region,
            activities.clientVersion,
            activities.assetVersion ? `资源 ${activities.assetVersion}` : null,
            activities.manifestVersion ? `目录 v${activities.manifestVersion}` : null,
        ]
            .filter(Boolean)
            .join(" ")
    elements.activityCatalogState.textContent = sourceSummary
        ? `${catalogSummary}, ${sourceSummary}`
        : catalogSummary
    if (visibleActivities.length === 0) {
        elements.activityCatalogList.append(createElement("p", "empty-state", "没有符合当前筛选条件的活动."))
    }
    for (const activity of visibleActivities) {
        elements.activityCatalogList.append(createActivityCard(activity, actions))
    }
    renderActivityDetail(activities, elements, actions)
}

function renderActivityQuickFilters(activities, elements, actions, resultCount) {
    const container = activityFilterContainer(elements, "activityQuickFilters", "activityQuickFilters")
    if (!container) return
    container.classList.add("activity-filter-chips")
    container.replaceChildren()

    const kindGroup = createFilterGroup("分类", "活动分类")
    kindGroup.append(createFilterChip("全部", activities.kind === "", () => {
        return actions.setActivityKindFilter("")
    }, actions.runAction))
    for (const kind of activities.knownKinds) {
        kindGroup.append(createFilterChip(activityKindLabel(kind), activities.kind === kind, () => {
            return actions.setActivityKindFilter(kind)
        }, actions.runAction))
    }

    const statusGroup = createFilterGroup("状态", "常用活动状态")
    for (const [status, label] of [["open", "进行中"], ["not_started", "即将开始"], ["ended", "已结束"]]) {
        statusGroup.append(createFilterChip(label, activities.status === status, () => {
            return actions.setActivityStatusFilter(activities.status === status ? "" : status)
        }, actions.runAction))
    }
    statusGroup.append(createFilterChip("收藏", activities.favoriteOnly, () => {
        return actions.setActivityFavoriteFilter(!activities.favoriteOnly)
    }, actions.runAction))

    const result = createElement("span", "activity-filter-result activity-filter-count", `显示 ${resultCount} 项`)
    result.setAttribute("aria-live", "polite")
    container.append(kindGroup, statusGroup, result)
    if (hasActivityFilters(activities)) {
        container.append(createFilterAction("清除筛选", actions.clearActivityFilters, actions.runAction))
    }
}

function renderActivityTagFilters(activities, elements, actions) {
    const container = activityFilterContainer(elements, "activityTagFilters", "activityTagFilters")
    if (!container) return
    const tags = activityTagChoices(activities)
    container.classList.add("activity-filter-chips")
    container.replaceChildren()
    container.hidden = tags.length === 0
    if (tags.length === 0) return

    container.append(createElement("span", "activity-filter-label", "标签"))
    container.append(createFilterChip("全部标签", activities.tag === "", () => {
        actions.setActivityTagFilter("")
    }, actions.runAction))
    for (const tag of tags) {
        container.append(createFilterChip(tag, activities.tag === tag, () => {
            actions.setActivityTagFilter(activities.tag === tag ? "" : tag)
        }, actions.runAction))
    }
}

function activityFilterContainer(elements, property, id) {
    return elements[property] ?? document.getElementById?.(id) ?? null
}

function createFilterGroup(label, ariaLabel) {
    const group = createElement("div", "activity-filter-group")
    group.setAttribute("role", "group")
    group.setAttribute("aria-label", ariaLabel)
    group.append(createElement("span", "activity-filter-label", label))
    return group
}

function createFilterChip(label, active, action, runAction) {
    const control = createElement("button", `activity-filter-chip${active ? " is-active" : ""}`, label)
    control.type = "button"
    control.setAttribute("aria-pressed", String(active))
    control.addEventListener("click", () => runAction(control, action))
    return control
}

function createFilterAction(label, action, runAction) {
    const control = createElement("button", "activity-filter-chip activity-filter-clear", label)
    control.type = "button"
    control.addEventListener("click", () => runAction(control, action))
    return control
}

function activityTagChoices(activities) {
    const counts = new Map()
    for (const activity of activities.items) {
        for (const tag of activity.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1)
    }
    if (activities.tag && !counts.has(activities.tag)) counts.set(activities.tag, 0)
    const tags = [...counts.keys()].sort((left, right) => {
        return (counts.get(right) - counts.get(left)) || left.localeCompare(right, "zh-CN")
    })
    const choices = tags.slice(0, 16)
    if (activities.tag && !choices.includes(activities.tag)) choices[choices.length - 1] = activities.tag
    return choices
}

function hasActivityFilters(activities) {
    return Boolean(
        activities.search
        || activities.kind
        || activities.status
        || activities.favoriteOnly
        || activities.tag
        || activities.dateFrom
        || activities.dateTo,
    )
}

function fillActivityKindFilter(activities, elements) {
    const selected = activities.kind
    elements.activityKindFilter.replaceChildren()
    const allKinds = createElement("option", "", "全部类型")
    allKinds.value = ""
    elements.activityKindFilter.append(allKinds)
    for (const kind of activities.knownKinds) {
        const option = createElement("option", "", activityKindLabel(kind))
        option.value = kind
        option.selected = kind === selected
        elements.activityKindFilter.append(option)
    }
}

function filterActivitiesByTag(activities) {
    if (!activities.tag) return activities.items
    return activities.items.filter((activity) => activity.tags.includes(activities.tag))
}

export function filterActivitiesByDate(activities, items) {
    if (activities.status === "ended") {
        return items.filter((activity) => activity.status === "ended")
    }
    const start = activities.dateFrom ? Date.parse(`${activities.dateFrom}T00:00:00.000Z`) : null
    const end = activities.dateTo ? Date.parse(`${activities.dateTo}T23:59:59.999Z`) : null
    if (start === null && end === null) return items
    return items.filter((activity) => {
        if (
            Number.isFinite(activity.temporary_open_until_ms)
            && Number.isFinite(activities.serverTimeMs)
            && activities.serverTimeMs >= (start ?? -8640000000000000)
            && activities.serverTimeMs <= (end ?? 8640000000000000)
        ) {
            return true
        }
        return activityOverlapsRange(activity, start ?? -8640000000000000, end ?? 8640000000000000)
    })
}

function createActivityCard(activity, actions) {
    const card = createElement("article", "activity-card")
    card.append(createActivityImage(activity, "activity-card-banner"))
    const content = createElement("div", "activity-card-content")
    const head = createElement("div", "card-head")
    const title = createElement("div")
    title.append(createElement("h3", "card-title", activity.name))
    title.append(createElement("p", "meta", `${activityKindLabel(activity.kind)} / ${activity.activity_id}`))
    head.append(title, createActivityStatusBadge(activity.status))
    content.append(head)
    if (activity.description) content.append(createElement("p", "activity-card-description", activity.description))
    content.append(createElement("p", "meta", activityWindowLabel(activity)))
    const controls = createElement("div", "card-actions")
    controls.append(createButton("查看详情", async () => {
        actions.selectActivity(activity.activity_id)
        return undefined
    }, actions.runAction))
    controls.append(createButton(activity.favorite ? "取消收藏" : "收藏", async () => {
        return actions.toggleActivityFavorite(activity)
    }, actions.runAction))
    const hasTemporaryOpen = Number.isFinite(activity.temporary_open_until_ms)
    if (hasTemporaryOpen) {
        controls.append(createButton("结束临时开放", async () => {
            return actions.temporaryActivityAction(activity, false)
        }, actions.runAction, "danger"))
    } else if (isUnderlyingActivityOpen(activity)) {
        controls.append(createButton("结束", async () => actions.closeActivity(activity), actions.runAction, "danger"))
    } else {
        controls.append(createButton("开放 24 小时", async () => {
            return actions.temporaryActivityAction(activity, true)
        }, actions.runAction, "primary"))
    }
    content.append(controls)
    card.append(content)
    return card
}

function renderActivityCalendar(activities, elements, actions, items) {
    const month = /^\d{4}-\d{2}$/.test(activities.calendarMonth)
        ? activities.calendarMonth
        : new Date().toISOString().slice(0, 7)
    const firstDay = new Date(`${month}-01T00:00:00.000Z`)
    const year = firstDay.getUTCFullYear()
    const monthIndex = firstDay.getUTCMonth()
    const dayCount = new Date(Date.UTC(year, monthIndex + 1, 0)).getUTCDate()
    const leadingDays = (firstDay.getUTCDay() + 6) % 7
    const currentDate = Number.isFinite(activities.serverTimeMs)
        ? new Date(activities.serverTimeMs).toISOString().slice(0, 10)
        : null
    elements.activityCalendarTitle.textContent = `${year} 年 ${monthIndex + 1} 月`
    elements.activityCalendar.replaceChildren()
    for (const weekday of ["一", "二", "三", "四", "五", "六", "日"]) {
        elements.activityCalendar.append(createElement("span", "activity-calendar-weekday", weekday))
    }
    for (let index = 0; index < leadingDays; index += 1) {
        elements.activityCalendar.append(createElement("span", "activity-calendar-spacer"))
    }
    for (let day = 1; day <= dayCount; day += 1) {
        const date = `${month}-${String(day).padStart(2, "0")}`
        const dayActivities = items.filter((activity) => {
            return (Number.isFinite(activity.temporary_open_until_ms) && date === currentDate)
                || activityOverlapsDate(activity, date)
        })
        const isSelected = activities.dateFrom === date && activities.dateTo === date
        const cell = createElement(
            "button",
            `activity-calendar-day${dayActivities.length ? " has-activity" : ""}${date === currentDate ? " is-current" : ""}${isSelected ? " is-selected" : ""}`,
        )
        cell.type = "button"
        cell.setAttribute("role", "gridcell")
        cell.setAttribute("aria-label", `${date}, ${dayActivities.length} 个活动`)
        if (date === currentDate) cell.setAttribute("aria-current", "date")
        cell.setAttribute("aria-pressed", String(isSelected))
        cell.append(createElement("span", "activity-calendar-number", String(day)))
        if (dayActivities.length) {
            cell.append(createElement("span", "activity-calendar-count", String(dayActivities.length)))
            cell.title = dayActivities.slice(0, 4).map((activity) => activity.name).join("\n")
        }
        cell.addEventListener("click", () => actions.setActivityDate(date))
        elements.activityCalendar.append(cell)
    }
}

export function renderActivityDetail(activities, elements, actions) {
    const activity = activities.items.find((item) => String(item.activity_id) === String(activities.selectedId))
    elements.activityDetail.hidden = !activity
    if (!activity) return
    elements.activityDetailTitle.textContent = activity.name
    elements.activityDetailMeta.textContent = `${activityKindLabel(activity.kind)} / ${activity.activity_id} / ${activityStatusLabel(activity.status)}`
    elements.activityDetailDescription.textContent = activity.description || "目录没有提供活动说明."
    elements.activityDetailTags.replaceChildren()
    for (const tag of activity.tags) elements.activityDetailTags.append(createElement("span", "activity-tag", tag))
    renderActivityDetailImage(activity, elements)
    elements.activityFavoriteButton.textContent = activity.favorite ? "取消收藏" : "收藏"
    const hasTemporaryOpen = Number.isFinite(activity.temporary_open_until_ms)
    const hasUnderlyingOpen = isUnderlyingActivityOpen(activity)
    elements.activityOpenButton.textContent = "开放 24 小时"
    elements.activityOpenButton.disabled = hasTemporaryOpen || hasUnderlyingOpen
    elements.activityCloseButton.textContent = hasTemporaryOpen ? "结束临时开放" : "快速结束"
    elements.activityCloseButton.disabled = !hasTemporaryOpen && !hasUnderlyingOpen
    elements.activityTemporaryState.textContent = hasTemporaryOpen
        ? "临时开放至 " + formatLocalTime(activity.temporary_open_until_ms) + " (本机时间)"
        : "当前没有临时开放."
    elements.activityWindowStart.value = activityInputTime(activityTime(activity, "start"))
    elements.activityWindowEnd.value = activityInputTime(activityTime(activity, "end"))
    elements.activityMode.value = activity.mode ?? activity.schedule?.mode ?? "manual"
    elements.activityPeriodKind.value = activity.period ?? activity.schedule?.period ?? "once"
    elements.activityPeriodInterval.value = String(activity.interval_days ?? activity.schedule?.interval_days ?? 1)
    elements.activityPeriodInterval.disabled = elements.activityPeriodKind.value !== "interval_days"
    elements.activityDetailClose.onclick = actions.closeActivityDetail
    elements.activityFavoriteButton.onclick = () => actions.runAction(
        elements.activityFavoriteButton,
        () => actions.toggleActivityFavorite(activity),
    )
    elements.activityOpenButton.onclick = () => actions.runAction(
        elements.activityOpenButton,
        () => actions.temporaryActivityAction(activity, true),
    )
    elements.activityCloseButton.onclick = () => actions.runAction(
        elements.activityCloseButton,
        () => hasTemporaryOpen
            ? actions.temporaryActivityAction(activity, false)
            : actions.closeActivity(activity),
    )
}

function renderActivityDetailImage(activity, elements) {
    const frame = elements.activityDetailBanner.parentElement
    renderActivityImage(
        activity,
        frame,
        elements.activityDetailBanner,
        elements.activityDetailBannerPlaceholder,
    )
}

function createActivityImage(activity, className) {
    const frame = createElement("figure", `activity-banner-frame ${className}`)
    const image = createElement("img")
    image.loading = "lazy"
    image.decoding = "async"
    const placeholder = createElement("span", "activity-banner-placeholder", "暂无活动图片")
    frame.append(image, placeholder)
    renderActivityImage(activity, frame, image, placeholder)
    return frame
}

function renderActivityImage(activity, frame, image, placeholder) {
    const candidates = activityImageCandidates(activity)
    let candidateIndex = 0
    image.alt = `${activity.name} 活动图片`
    image.decoding = "async"
    image.loading = "lazy"
    image.onerror = null
    image.removeAttribute("src")

    const loadNextCandidate = () => {
        const candidate = candidates[candidateIndex++]
        if (!candidate) {
            image.hidden = true
            placeholder.hidden = false
            frame.style.aspectRatio = ""
            delete frame.dataset.imageSource
            frame.removeAttribute("title")
            return
        }
        const sourceLabel = activityImageSourceLabel(candidate.source_type)
        frame.style.aspectRatio = activityImageAspectRatio(candidate) ?? ""
        frame.dataset.imageSource = sourceLabel
        if (candidate.evidence) frame.title = `${sourceLabel}: ${candidate.evidence}`
        else frame.removeAttribute("title")
        image.hidden = false
        placeholder.hidden = true
        image.onerror = () => {
            if (image.getAttribute("src") !== candidate.url) return
            loadNextCandidate()
        }
        image.src = candidate.url
    }

    loadNextCandidate()
}

function activityImageCandidates(activity) {
    const candidates = Array.isArray(activity.image_candidates) ? activity.image_candidates : []
    const normalized = candidates.flatMap((candidate) => {
        const url = safeActivityImageUrl(candidate?.url)
        if (!url) return []
        return [{
            evidence: String(candidate.evidence ?? ""),
            height: Number(candidate.height) || null,
            source_type: String(candidate.source_type ?? "unclassified"),
            url,
            width: Number(candidate.width) || null,
        }]
    })
    const legacyUrl = safeActivityImageUrl(activity.banner_url)
    if (legacyUrl && !normalized.some((candidate) => candidate.url === legacyUrl)) {
        normalized.unshift({
            evidence: String(activity.banner_evidence ?? ""),
            height: Number(activity.banner_height) || null,
            source_type: String(activity.banner_source_type ?? "activity_banner"),
            url: legacyUrl,
            width: Number(activity.banner_width) || null,
        })
    }
    return normalized
}

function activityImageAspectRatio(image) {
    const width = Number(image.width)
    const height = Number(image.height)
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null
    return `${width} / ${height}`
}

function activityImageSourceLabel(sourceType) {
    return {
        activity_banner: "活动 banner",
        dedicated_banner: "活动 banner",
        event_banner: "活动 banner",
        home_banner: "首页滚动图",
        home_carousel: "首页滚动图",
        notice_banner: "公告图",
        activity_entry: "活动入口图",
        event_entry: "活动入口图",
        boss_cover: "Boss 封面",
        quest_cover: "关卡封面",
        stage_cover: "关卡封面",
        exchange_icon: "交换所图标",
        mission_icon: "任务图标",
        shop_icon: "商店图标",
        shop_exchange: "商店或交换所图片",
        activity_logo: "活动 logo",
        activity_background: "活动背景",
        master_image: "活动附属图片",
        type_placeholder: "类型占位图",
        unclassified: "图片来源未标注",
    }[sourceType] ?? sourceType ?? "图片来源未标注"
}

function safeActivityImageUrl(value) {
    if (typeof value !== "string" || !value) return null
    try {
        const url = new URL(value, location.origin)
        if (url.origin !== location.origin) return null
        if (!url.pathname.startsWith("/manage/assets/activity-banners/")) return null
        return `${url.pathname}${url.search}`
    } catch {
        return null
    }
}

function activityTime(activity, boundary) {
    const names = boundary === "start"
        ? ["start_at_ms", "start_at", "default_start_at_ms", "default_start_at", "next_start_at_ms", "next_start_at"]
        : ["end_at_ms", "end_at", "default_end_at_ms", "default_end_at", "next_end_at_ms", "next_end_at"]
    for (const name of names) {
        const value = activity[name] ?? activity.schedule?.[name]
        if (typeof value === "number" && Number.isFinite(value)) return value
        if (typeof value === "string" && value) {
            const parsed = Date.parse(value)
            if (Number.isFinite(parsed)) return parsed
        }
    }
    return null
}

function activityOverlapsDate(activity, date) {
    const startOfDay = Date.parse(`${date}T00:00:00.000Z`)
    const endOfDay = startOfDay + 86400000 - 1
    return activityOverlapsRange(activity, startOfDay, endOfDay)
}

// //// 按活动周期判断日期范围是否有活动窗口 [@x380kkm 2026-08-19] ////
function activityOverlapsRange(activity, rangeStart, rangeEnd) {
    const mode = activity.mode ?? activity.schedule?.mode ?? "window"
    if (mode === "always") return true
    if (mode === "manual") return false
    if (activity.enabled === false || activity.schedule?.enabled === false || activity.status === "disabled") return false
    const start = activityNumber(activity, ["start_at_ms", "default_start_at_ms", "start_at", "default_start_at"])
    const end = activityNumber(activity, ["end_at_ms", "default_end_at_ms", "end_at", "default_end_at"])
    if (start === null || end === null || end <= start || rangeEnd < rangeStart) return false
    const period = activity.period ?? activity.schedule?.period ?? "once"
    if (mode !== "periodic" || period === "once") return start <= rangeEnd && end > rangeStart
    const duration = end - start
    if (!Number.isFinite(duration) || duration <= 0) return false
    if (period === "monthly") return monthlyActivityOverlapsRange(start, duration, rangeStart, rangeEnd)
    const periodMs = activityPeriodMilliseconds(activity, period)
    if (periodMs === null) return start <= rangeEnd && end > rangeStart
    if (rangeEnd < start) return false
    const effectiveStart = Math.max(rangeStart, start)
    if (rangeEnd - effectiveStart >= periodMs) return true
    const firstIndex = Math.max(0, Math.floor((effectiveStart - start - duration) / periodMs))
    const lastIndex = Math.max(0, Math.floor((rangeEnd - start) / periodMs))
    if (lastIndex < firstIndex) return false
    for (let index = firstIndex; index <= lastIndex; index += 1) {
        const occurrenceStart = start + index * periodMs
        if (occurrenceStart <= rangeEnd && occurrenceStart + duration > rangeStart) return true
    }
    return false
}

function activityNumber(activity, names) {
    for (const name of names) {
        const value = activity[name] ?? activity.schedule?.[name]
        if (typeof value === "number" && Number.isFinite(value)) return value
        if (typeof value === "string" && value) {
            const parsed = Date.parse(value)
            if (Number.isFinite(parsed)) return parsed
        }
    }
    return null
}

function activityPeriodMilliseconds(activity, period) {
    if (period === "daily") return 86400000
    if (period === "weekly") return 7 * 86400000
    if (period !== "interval_days") return null
    const intervalDays = Number(activity.interval_days ?? activity.schedule?.interval_days)
    return Number.isInteger(intervalDays) && intervalDays > 0 ? intervalDays * 86400000 : null
}

function monthlyActivityOverlapsRange(start, duration, rangeStart, rangeEnd) {
    const startDate = new Date(start)
    const rangeStartDate = new Date(rangeStart)
    const rangeEndDate = new Date(rangeEnd)
    if ([startDate, rangeStartDate, rangeEndDate].some((date) => Number.isNaN(date.valueOf()))) return false
    const startOrdinal = startDate.getUTCFullYear() * 12 + startDate.getUTCMonth()
    const rangeStartOrdinal = rangeStartDate.getUTCFullYear() * 12 + rangeStartDate.getUTCMonth()
    const currentIndex = Math.max(0, rangeStartOrdinal - startOrdinal)
    const firstIndex = Math.max(0, currentIndex - 1)
    const lastIndex = currentIndex + 1
    for (let index = firstIndex; index <= lastIndex; index += 1) {
        const occurrenceStart = addUtcMonths(startDate, index)
        if (occurrenceStart.getTime() <= rangeEnd && occurrenceStart.getTime() + duration > rangeStart) return true
    }
    return false
}

function addUtcMonths(date, months) {
    const target = new Date(Date.UTC(
        date.getUTCFullYear(),
        date.getUTCMonth() + months,
        1,
        date.getUTCHours(),
        date.getUTCMinutes(),
        date.getUTCSeconds(),
        date.getUTCMilliseconds(),
    ))
    const lastDay = new Date(Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0)).getUTCDate()
    target.setUTCDate(Math.min(date.getUTCDate(), lastDay))
    return target
}
// //// /按活动周期判断日期范围是否有活动窗口 ////

function activityWindowLabel(activity) {
    const start = activityTime(activity, "start")
    const end = activityTime(activity, "end")
    if (start === null && end === null) return "未设置活动时间"
    if (start !== null && end !== null) return `${formatUtcTime(start)} 至 ${formatUtcTime(end)}`
    return start !== null ? `${formatUtcTime(start)} 开始` : `${formatUtcTime(end)} 结束`
}

function activityInputTime(value) {
    if (value === null) return ""
    const date = new Date(value)
    return Number.isNaN(date.valueOf()) ? "" : date.toISOString().slice(0, 19)
}

function formatUtcTime(value) {
    const date = new Date(value)
    return Number.isNaN(date.valueOf()) ? String(value) : date.toISOString().replace(".000Z", "Z")
}

function formatLocalTime(value) {
    const date = new Date(value)
    return Number.isNaN(date.valueOf()) ? String(value) : date.toLocaleString(undefined, { hour12: false })
}

function createActivityStatusBadge(status) {
    return createElement("span", `badge activity-status ${status}`, activityStatusLabel(status))
}

function isUnderlyingActivityOpen(activity) {
    const status = activity.underlying_status ?? activity.status
    return status === "open" || status === "unscheduled"
}

function activityStatusLabel(status) {
    return {
        disabled: "已关闭",
        ended: "已结束",
        not_started: "未开始",
        open: "进行中",
        unscheduled: "未排期",
    }[status] ?? status
}

function activityKindLabel(kind) {
    return {
        "active-mission": "限时任务",
        advent: "Advent 活动",
        "box-gacha": "Box Gacha",
        carnival: "Carnival 活动",
        challenge: "挑战关卡",
        "collect-item": "收集活动",
        daily: "每日轮换",
        event: "活动",
        "event-shop": "活动商店",
        expert: "高难关卡",
        gacha: "卡池",
        "gacha-campaign": "抽卡活动",
        multi: "高难共斗",
        other: "其他",
        "pass-card": "Pass Card",
        raid: "Raid",
        ranking: "排名战",
        rush: "Rush 活动",
        "score-attack": "Score Attack",
        story: "剧情",
        "time-attack": "Time Attack",
        tower: "爬塔",
        "world-story": "World Story",
    }[kind] ?? kind
}
// //// /呈现活动目录和图片 ////
