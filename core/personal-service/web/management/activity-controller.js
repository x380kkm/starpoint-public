// audience: external
// # personal-service-activity-controller
//
// 此模块管理活动目录状态、活动 API 调用和活动表单事件. 调用方提供同源请求函数和通用操作反馈.

import { renderActivityCatalog, renderActivityDetail } from "/manage/activity-views.js"

// //// 管理活动目录和活动规则 [@x380kkm 2026-08-19] ////
export function createActivityController({ elements, requestApi, ApiError, runAction }) {
    const state = {
        calendarInitialized: false,
        calendarMonth: new Date().toISOString().slice(0, 7),
        dateFrom: "",
        dateTo: "",
        defaultDateInitialized: false,
        error: null,
        favoriteOnly: false,
        items: [],
        kind: "",
        knownKinds: [],
        manifestState: "unknown",
        manifestVersion: null,
        assetVersion: null,
        region: null,
        clientVersion: null,
        selectedId: null,
        serverTimeMs: null,
        status: "",
        search: "",
        tag: "",
    }
    let catalogGeneration = 0
    let searchTimer

    function catalogQuery() {
        const query = new URLSearchParams()
        if (state.search) query.set("q", state.search)
        if (state.kind) query.set("kind", state.kind)
        if (state.status) query.set("status", state.status)
        if (state.favoriteOnly) query.set("favorite", "true")
        return query.toString()
    }

    function catalogPath() {
        const query = new URLSearchParams(catalogQuery())
        const suffix = query.toString()
        return suffix ? `/v1/activities/catalog?${suffix}` : "/v1/activities/catalog"
    }

    function normalizeImageCandidate(candidate) {
        const source = candidate ?? {}
        const url = source.url ?? source.banner_url ?? source.bannerUrl ?? ""
        if (!url) return null
        return {
            ...source,
            evidence: String(source.evidence ?? ""),
            height: Number(source.height ?? source.banner_height ?? source.bannerHeight) || null,
            source_type: String(source.source_type ?? source.sourceType ?? "unclassified"),
            url: String(url),
            width: Number(source.width ?? source.banner_width ?? source.bannerWidth) || null,
        }
    }

    function normalizeEntry(entry, index) {
        const source = entry ?? {}
        const id = String(source.activity_id ?? source.id ?? source.event_id ?? `activity-${index + 1}`)
        const sourceTags = Array.isArray(source.tags)
            ? source.tags.map((tag) => String(tag))
            : typeof source.tags === "string"
                ? source.tags.split(",").map((tag) => tag.trim()).filter(Boolean)
                : []
        const tags = [...new Set(sourceTags.map((tag) => tag.trim()).filter(Boolean))]
        const imageCandidates = Array.isArray(source.image_candidates)
            ? source.image_candidates.map(normalizeImageCandidate).filter(Boolean)
            : []
        const legacyCandidate = normalizeImageCandidate({
            evidence: source.banner_evidence,
            height: source.banner_height ?? source.bannerHeight,
            source_type: source.banner_source_type ?? source.bannerSourceType ?? "activity_banner",
            url: source.banner_url ?? source.bannerUrl ?? source.banner?.url,
            width: source.banner_width ?? source.bannerWidth,
        })
        if (legacyCandidate && !imageCandidates.some((candidate) => candidate.url === legacyCandidate.url)) {
            imageCandidates.unshift(legacyCandidate)
        }
        const primaryImage = imageCandidates[0] ?? legacyCandidate
        const temporaryOpenUntilMs = Number(source.temporary_open_until_ms ?? source.temporaryOpenUntilMs)
        return {
            ...source,
            activity_id: id,
            banner_evidence: primaryImage?.evidence ?? "",
            banner_height: primaryImage?.height ?? null,
            banner_source_type: primaryImage?.source_type ?? "",
            banner_url: primaryImage?.url ?? "",
            banner_width: primaryImage?.width ?? null,
            default_end_at: source.default_end_at ?? source.defaultEndAt ?? null,
            default_start_at: source.default_start_at ?? source.defaultStartAt ?? null,
            description: String(source.description ?? ""),
            favorite: Boolean(source.favorite ?? source.is_favorite),
            image_candidates: imageCandidates,
            kind: String(source.kind ?? source.type ?? "other"),
            name: String(source.name ?? source.title ?? id),
            underlying_status: String(
                source.underlying_status ?? source.underlyingStatus ?? source.status ?? "unscheduled",
            ),
            status: String(source.status ?? "unscheduled"),
            tags,
            temporary_open_until_ms: Number.isFinite(temporaryOpenUntilMs) && temporaryOpenUntilMs > 0
                ? temporaryOpenUntilMs
                : null,
        }
    }

    function normalizeCatalog(payload) {
        const entries = Array.isArray(payload)
            ? payload
            : payload?.activities ?? payload?.catalog ?? payload?.items ?? payload?.schedules ?? []
        return {
            clientVersion: payload?.client_version ?? null,
            assetVersion: payload?.asset_version ?? null,
            formatVersion: payload?.format_version ?? null,
            items: entries.map(normalizeEntry),
            manifestState: payload?.manifest_state ?? "unknown",
            region: payload?.region ?? null,
            serverTimeMs: payload?.server_time_ms ?? null,
        }
    }

    function setCurrentTime(timeMs) {
        const normalizedTimeMs = Number(timeMs)
        if (!Number.isFinite(normalizedTimeMs) || normalizedTimeMs <= 0) return
        state.serverTimeMs = normalizedTimeMs
        if (state.calendarInitialized) return
        state.calendarMonth = new Date(normalizedTimeMs).toISOString().slice(0, 7)
        state.calendarInitialized = true
    }

    function selectCurrentDate() {
        if (!Number.isFinite(state.serverTimeMs)) return
        const date = new Date(state.serverTimeMs).toISOString().slice(0, 10)
        state.dateFrom = date
        state.dateTo = date
        elements.activityDateFrom.value = date
        elements.activityDateTo.value = date
        state.defaultDateInitialized = true
    }

    function selectDefaultDate() {
        if (state.defaultDateInitialized) return
        selectCurrentDate()
    }

    async function load() {
        const generation = ++catalogGeneration
        try {
            const payload = await requestApi(catalogPath())
            const catalog = normalizeCatalog(payload)
            if (generation !== catalogGeneration) return
            state.items = catalog.items
            state.knownKinds = [...new Set([
                ...state.knownKinds,
                ...catalog.items.map((activity) => activity.kind),
            ])].sort((left, right) => left.localeCompare(right))
            state.error = null
            state.manifestState = catalog.manifestState
            state.manifestVersion = catalog.formatVersion
            state.region = catalog.region
            state.clientVersion = catalog.clientVersion
            state.assetVersion = catalog.assetVersion
            setCurrentTime(catalog.serverTimeMs)
            if (state.status === "ended") selectCurrentDate()
            else selectDefaultDate()
        } catch (error) {
            if (generation !== catalogGeneration) return
            state.items = []
            state.error = error
        }
        render()
    }

    function render() {
        const actions = {
            closeActivity,
            closeActivityDetail: () => {
                state.selectedId = null
                renderActivityDetail(state, elements, actions)
            },
            runAction,
            selectActivity: (activityId) => {
                state.selectedId = String(activityId)
                renderActivityDetail(state, elements, actions)
                elements.activityDetail.scrollIntoView?.({ block: "nearest" })
            },
            setActivityDate: setActivityDate,
            setActivityFavoriteFilter,
            setActivityKindFilter,
            setActivityStatusFilter,
            setActivityTagFilter,
            clearActivityFilters,
            temporaryActivityAction,
            toggleActivityFavorite,
        }
        renderActivityCatalog(state, elements, actions)
    }

    function selectedActivity() {
        return state.items.find((item) => String(item.activity_id) === String(state.selectedId))
    }

    function setActivityDate(date) {
        state.dateFrom = date
        state.dateTo = date
        state.defaultDateInitialized = true
        elements.activityDateFrom.value = date
        elements.activityDateTo.value = date
        render()
    }

    async function calendarFallback(activityId, changes) {
        let current = {}
        try {
            current = await requestApi(`/v1/activities/calendar/${encodeURIComponent(activityId)}`)
        } catch (error) {
            if (!(error instanceof ApiError) || error.status !== 404) throw error
        }
        const startAt = changes.start_at ?? changes.start_at_ms ?? current.start_at ?? current.start_at_ms
        const endAt = changes.end_at ?? changes.end_at_ms ?? current.end_at ?? current.end_at_ms
        if (startAt === undefined || endAt === undefined || startAt === null || endAt === null) {
            throw new ApiError(400, "activity_action_unavailable")
        }
        return requestApi(`/v1/activities/calendar/${encodeURIComponent(activityId)}`, {
            method: "PUT",
            body: {
                enabled: changes.enabled ?? Boolean(current.enabled),
                start_at: typeof startAt === "number" ? undefined : startAt,
                start_at_ms: typeof startAt === "number" ? startAt : undefined,
                end_at: typeof endAt === "number" ? undefined : endAt,
                end_at_ms: typeof endAt === "number" ? endAt : undefined,
            },
        })
    }

    async function mutate(activityId, path, body, fallbackChanges) {
        try {
            return await requestApi(`/v1/activities/${encodeURIComponent(activityId)}${path}`, {
                method: "POST",
                body,
            })
        } catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status)) throw error
            return calendarFallback(activityId, fallbackChanges)
        }
    }

    async function closeActivity(activity) {
        const id = String(activity.activity_id)
        const defaultWindow = {
            start_at_ms: activity.start_at_ms ?? activity.default_start_at_ms ?? 0,
            end_at_ms: activity.end_at_ms ?? activity.default_end_at_ms ?? 253402300799000,
        }
        await mutate(
            id,
            "/close",
            {},
            { enabled: false, ...defaultWindow },
        )
        await load()
        return "活动已结束."
    }

    async function temporaryActivityAction(activity, shouldOpen) {
        const id = encodeURIComponent(String(activity.activity_id))
        await requestApi("/v1/activities/" + id + "/temporary-open", {
            method: shouldOpen ? "POST" : "DELETE",
            body: shouldOpen ? {} : undefined,
        })
        await load()
        return shouldOpen ? "活动已开放 24 小时." : "临时开放已结束."
    }

    async function toggleActivityFavorite(activity) {
        const id = encodeURIComponent(String(activity.activity_id))
        const nextFavorite = !activity.favorite
        try {
            await requestApi(`/v1/activities/catalog/${id}/favorite`, {
                method: nextFavorite ? "PUT" : "DELETE",
            })
        } catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status)) throw error
            await requestApi("/v1/activities/catalog/favorite", {
                method: "PUT",
                body: { activity_id: String(activity.activity_id), favorite: nextFavorite },
            })
        }
        await load()
        return nextFavorite ? "已收藏活动." : "已取消收藏."
    }

    async function updateWindow(activity, startAt, endAt) {
        const id = String(activity.activity_id)
        try {
            await requestApi(`/v1/activities/${encodeURIComponent(id)}/window`, {
                method: "PUT",
                body: { start_at: startAt || null, end_at: endAt || null },
            })
        } catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status)) throw error
            await calendarFallback(id, { enabled: true, start_at: startAt, end_at: endAt })
        }
        await load()
        return "活动时间窗口已保存."
    }

    async function updateMode(activity, mode) {
        const id = encodeURIComponent(String(activity.activity_id))
        try {
            await requestApi(`/v1/activities/${id}/mode`, { method: "PUT", body: { mode } })
        } catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status)) throw error
            if (mode === "always" || mode === "manual") {
                await calendarFallback(activity.activity_id, {
                    enabled: mode === "always",
                    start_at_ms: activity.start_at_ms ?? activity.default_start_at_ms ?? 0,
                    end_at_ms: activity.end_at_ms ?? activity.default_end_at_ms ?? 253402300799000,
                })
            } else {
                throw new ApiError(error.status, "activity_action_unavailable")
            }
        }
        await load()
        return "活动模式已保存."
    }

    async function updatePeriod(activity, period, intervalDays) {
        const id = String(activity.activity_id)
        const body = { period }
        if (period === "interval_days") {
            if (!Number.isInteger(intervalDays) || intervalDays < 1 || intervalDays > 3650) {
                throw new Error("间隔天数必须位于 1 到 3650.")
            }
            body.interval_days = intervalDays
        }
        try {
            await requestApi(`/v1/activities/${encodeURIComponent(id)}/period`, { method: "PUT", body })
        } catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status)) throw error
            throw new ApiError(error.status, "activity_action_unavailable")
        }
        await load()
        return "活动周期已保存."
    }

    function refreshFromFilters() {
        state.search = elements.activitySearch.value.trim()
        state.kind = elements.activityKindFilter.value
        state.status = elements.activityStatusFilter.value
        state.favoriteOnly = elements.activityFavoriteFilter.checked
        return load()
    }

    function setActivityKindFilter(kind) {
        state.kind = String(kind ?? "")
        elements.activityKindFilter.value = state.kind
        return load()
    }

    function setActivityStatusFilter(status) {
        state.status = String(status ?? "")
        elements.activityStatusFilter.value = state.status
        return load()
    }

    function setActivityFavoriteFilter(favoriteOnly) {
        state.favoriteOnly = Boolean(favoriteOnly)
        elements.activityFavoriteFilter.checked = state.favoriteOnly
        return load()
    }

    function setActivityTagFilter(tag) {
        state.tag = String(tag ?? "")
        render()
    }

    function clearActivityFilters() {
        clearTimeout(searchTimer)
        state.search = ""
        state.kind = ""
        state.status = ""
        state.favoriteOnly = false
        state.tag = ""
        elements.activitySearch.value = ""
        elements.activityKindFilter.value = ""
        elements.activityStatusFilter.value = ""
        elements.activityFavoriteFilter.checked = false
        clearDateRange()
        return load()
    }

    function clearDateRange() {
        state.dateFrom = ""
        state.dateTo = ""
        elements.activityDateFrom.value = ""
        elements.activityDateTo.value = ""
    }

    function changeCalendarMonth(delta) {
        const current = /^\d{4}-\d{2}$/.test(state.calendarMonth)
            ? new Date(`${state.calendarMonth}-01T00:00:00.000Z`)
            : new Date()
        current.setUTCMonth(current.getUTCMonth() + delta)
        state.calendarInitialized = true
        state.calendarMonth = `${current.getUTCFullYear()}-${String(current.getUTCMonth() + 1).padStart(2, "0")}`
        render()
    }

    async function resetActivities() {
        const result = await requestApi("/v1/activities/reset", {
            method: "POST",
            body: {},
        })
        await load()
        const resetCount = Number(result.reset_schedule_count || 0)
            + Number(result.reset_temporary_open_count || 0)
        return `已恢复包内活动时间, 清除 ${resetCount} 项设置.`
    }

    function bindEvents() {
        elements.activityCatalogRefresh.addEventListener("click", () => runAction(
            elements.activityCatalogRefresh,
            refreshFromFilters,
        ))
        elements.activityReset.addEventListener("click", () => runAction(
            elements.activityReset,
            resetActivities,
        ))
        elements.activitySearch.addEventListener("input", () => {
            clearTimeout(searchTimer)
            searchTimer = setTimeout(refreshFromFilters, 280)
        })
        elements.activityKindFilter.addEventListener("change", refreshFromFilters)
        elements.activityStatusFilter.addEventListener("change", refreshFromFilters)
        elements.activityFavoriteFilter.addEventListener("change", refreshFromFilters)
        elements.activityDateFrom.addEventListener("change", () => {
            state.dateFrom = elements.activityDateFrom.value
            state.defaultDateInitialized = true
            if (state.dateTo && state.dateFrom > state.dateTo) {
                state.dateTo = state.dateFrom
                elements.activityDateTo.value = state.dateTo
            }
            render()
        })
        elements.activityDateTo.addEventListener("change", () => {
            state.dateTo = elements.activityDateTo.value
            state.defaultDateInitialized = true
            if (state.dateFrom && state.dateTo < state.dateFrom) {
                state.dateFrom = state.dateTo
                elements.activityDateFrom.value = state.dateFrom
            }
            render()
        })
        elements.activityDateClear.addEventListener("click", () => {
            clearDateRange()
            render()
        })
        elements.activityCalendarPrevious.addEventListener("click", () => runAction(
            elements.activityCalendarPrevious,
            () => changeCalendarMonth(-1),
        ))
        elements.activityCalendarPreviousYear.addEventListener("click", () => runAction(
            elements.activityCalendarPreviousYear,
            () => changeCalendarMonth(-12),
        ))
        elements.activityCalendarNext.addEventListener("click", () => runAction(
            elements.activityCalendarNext,
            () => changeCalendarMonth(1),
        ))
        elements.activityCalendarNextYear.addEventListener("click", () => runAction(
            elements.activityCalendarNextYear,
            () => changeCalendarMonth(12),
        ))
        elements.activityCalendarToday.addEventListener("click", () => runAction(
            elements.activityCalendarToday,
            () => {
                const now = new Date(state.serverTimeMs ?? Date.now())
                state.calendarInitialized = true
                state.calendarMonth = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`
                setActivityDate(now.toISOString().slice(0, 10))
            },
        ))
        elements.activityWindowForm.addEventListener("submit", async (event) => {
            event.preventDefault()
            const activity = selectedActivity()
            if (!activity) return
            await runAction(event.submitter, () => updateWindow(
                activity,
                elements.activityWindowStart.value
                    ? new Date(`${elements.activityWindowStart.value}Z`).toISOString()
                    : null,
                elements.activityWindowEnd.value
                    ? new Date(`${elements.activityWindowEnd.value}Z`).toISOString()
                    : null,
            ))
        })
        elements.activityModeForm.addEventListener("submit", async (event) => {
            event.preventDefault()
            const activity = selectedActivity()
            if (!activity) return
            await runAction(event.submitter, () => updateMode(activity, elements.activityMode.value))
        })
        elements.activityPeriodForm.addEventListener("submit", async (event) => {
            event.preventDefault()
            const activity = selectedActivity()
            if (!activity) return
            await runAction(event.submitter, () => updatePeriod(
                activity,
                elements.activityPeriodKind.value,
                Number(elements.activityPeriodInterval.value),
            ))
        })
        elements.activityPeriodKind.addEventListener("change", () => {
            elements.activityPeriodInterval.disabled = elements.activityPeriodKind.value !== "interval_days"
        })
    }

    bindEvents()
    return {
        load,
        resetCalendar() {
            state.calendarInitialized = false
            state.defaultDateInitialized = false
            clearDateRange()
        },
        setCurrentTime,
    }
}
// //// /管理活动目录和活动规则 ////
