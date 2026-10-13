// audience: internal
// # personal-service-reward-history-view-tests
// 商品卡片区分内容发放和完成界面确认, 翻页沿用服务端游标.

import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import test from "node:test"

const source = await readFile(new URL("./reward-history.js", import.meta.url), "utf8")
const { createRewardHistory } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`)

function element() {
    return {
        value: "", textContent: "", hidden: false, children: [], events: {},
        get options() { return this.children },
        addEventListener(name, handler) { this.events[name] = handler },
        replaceChildren(...children) { this.children = children },
        append(...children) { this.children.push(...children) },
    }
}

test("已发放资格与完成界面确认分别显示并按订单游标翻页", async () => {
    const controls = Object.fromEntries(["save", "view", "state", "list", "refresh", "more"].map((key) => [key, element()]))
    controls.view.value = "products"
    const calls = []
    const controller = createRewardHistory({
        document: { querySelector: (id) => controls[id.replace("#reward-history-", "")], createElement: element },
        runAction: async (_, operation) => operation(),
        requestApi: async (path) => {
            calls.push(path)
            const next = path.includes("before_id")
            return { slot_name: "存档 A", next_before_id: next ? null : 6, entries: [{
                id: next ? 5 : 6, product_id: "com.leiting.wf.pass_card", entitlement_kinds: ["pass_card"],
                created_at: 1704067200, confirmed_at: next ? 1704067300 : null,
            }] }
        },
    })
    await controller.setSaves([{ id: 7, name: "存档 A" }], 7)
    const pendingText = controls.list.children[0].children.map((child) => child.textContent).join("\n")
    assert.match(pendingText, /商品内容已发放/)
    assert.match(pendingText, /通行证高阶奖励资格/)
    assert.match(pendingText, /完成界面待确认/)
    assert.doesNotMatch(pendingText, /未领取|待支付/)
    await controls.more.events.click()
    assert.deepEqual(calls, ["/v1/local-saves/7/product-history", "/v1/local-saves/7/product-history?before_id=6"])
    assert.equal(controls.list.children.length, 2)
    assert.match(controls.list.children[1].children.map((child) => child.textContent).join("\n"), /完成界面已确认/)
    assert.equal(controls.more.hidden, true)
})
