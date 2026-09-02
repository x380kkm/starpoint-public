// audience: internal
// # server-transfer-binding-runner
//
// 此模块在完整服务器前台生命周期内执行到期的 interval 绑定.
// 同一进程同时最多执行一个远端传输, stop() 等待当前请求结束.

import { listDueServerTransferBindingIdsSync } from "./serverTransferBindingStore"
import { synchronizeServerTransferBinding } from "./serverTransferBindingService"

type SynchronizeBinding = typeof synchronizeServerTransferBinding
type ListDueBindings = typeof listDueServerTransferBindingIdsSync

function getPollIntervalMilliseconds(): number {
    const value = Number(process.env.SERVER_TRANSFER_POLL_INTERVAL_MS ?? 1_000)
    return Number.isInteger(value) && value >= 100 && value <= 60_000 ? value : 1_000
}

// //// 轮询并执行一个到期绑定 [@x380kkm 2026-08-04] ////
export class ServerTransferBindingRunner {
    private timer: NodeJS.Timeout | null = null
    private active: Promise<void> | null = null
    private stopped = true

    constructor(
        private readonly synchronize: SynchronizeBinding = synchronizeServerTransferBinding,
        private readonly listDue: ListDueBindings = listDueServerTransferBindingIdsSync,
    ) {}

    start(): void {
        if (!this.stopped) return
        this.stopped = false
        this.schedule(0)
    }

    async stop(): Promise<void> {
        this.stopped = true
        if (this.timer !== null) clearTimeout(this.timer)
        this.timer = null
        await this.active
    }

    async pollOnce(now: Date = new Date()): Promise<boolean> {
        if (this.active !== null) return false
        const bindingId = this.listDue(now)[0]
        if (bindingId === undefined) return false
        this.active = this.synchronize(bindingId, "interval", "auto")
            .then(() => undefined)
            .catch(() => undefined)
            .finally(() => {
                this.active = null
            })
        await this.active
        return true
    }

    private schedule(delayMilliseconds: number): void {
        if (this.stopped) return
        this.timer = setTimeout(async () => {
            try {
                await this.pollOnce()
            } catch (error) {
                console.error("Server transfer binding runner poll failed.", error)
            }
            this.schedule(getPollIntervalMilliseconds())
        }, delayMilliseconds)
        this.timer.unref()
    }
}
// //// /轮询并执行一个到期绑定 ////
