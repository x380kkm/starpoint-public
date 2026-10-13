// audience: internal
// # ai-team-source-controller-tests
// 隔离 DOM 验证来源回显, 所选存档保存和异步切换时的候选归属.

import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'

class Element {
    constructor(tagName = 'div') {
        this.tagName = tagName
        this.children = []
        this.listeners = {}
        this.selected = false
        this._value = ''
    }
    append(...nodes) { this.children.push(...nodes) }
    replaceChildren(...nodes) { this.children = nodes }
    addEventListener(name, handler) { this.listeners[name] = handler }
    get options() { return this.children }
    set value(value) {
        this._value = value
        if (this.tagName === 'select') this.children.forEach(child => { child.selected = child.value === value })
    }
    get value() {
        if (this.tagName !== 'select') return this._value
        return (this.children.find(child => child.selected) ?? this.children[0])?.value ?? ''
    }
}

const source = await readFile(new URL('../../core/personal-service/web/management/ai-team-controller.js', import.meta.url), 'utf8')
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2017, module: ts.ModuleKind.CommonJS } }).outputText
const context = vm.createContext({ exports: {}, document: { createElement: tag => new Element(tag) } })
vm.runInContext(compiled, context)
const elements = Object.fromEntries(['A', 'B', 'Slot'].map(name => [`aiTeam${name}`, new Element('select')]))
for (const name of ['Candidates', 'Default', 'Follow', 'Save', 'Source', 'State']) elements[`aiTeam${name}`] = new Element()
const calls = []
let selectedSource = 22
let delayedResponse
let delaySlot
let notifyDelayedRequest
const sourceStatus = () => ({ slot_id: selectedSource, mode: selectedSource ? 'selected' : 'current' })
function selection(slot) {
    return {
        slot_id: slot,
        source: sourceStatus(),
        selected_party_ids: [slot * 10 + 1, slot * 10 + 2],
        candidates: [1, 2].map(index => ({ party_id: slot * 10 + index, name: `Save ${slot} party ${index}`, character_ids: [231001] })),
        selection_status: 'ready',
    }
}
const controller = context.exports.createAiTeamController({
    elements,
    runAction: (_control, action) => action(),
    requestApi: async (path, options = {}) => {
        calls.push({ path, ...options })
        if (path === '/v1/local-saves/ai-team-source') {
            if (options.method === 'DELETE') selectedSource = null
            return sourceStatus()
        }
        const slot = Number(path.match(/local-saves\/(\d+)/)[1])
        if (options.method === 'PUT') selectedSource = slot
        if (slot === delaySlot && !options.method) return new Promise(resolve => {
            delayedResponse = () => resolve(selection(slot))
            notifyDelayedRequest()
        })
        return selection(slot)
    },
})

// //// 打开页面还原所选来源并让保存和恢复操作使用明确的存档 [@x380kkm 2026-09-29] ////
controller.setSlots([{ id: 11, name: '当前玩家' }, { id: 22, name: 'AI 存档' }], 11)
await controller.load()
assert.equal(elements.aiTeamSlot.value, '22')
assert.equal(elements.aiTeamA.value, '221')
assert.match(elements.aiTeamSource.textContent, /AI 存档/)
elements.aiTeamA.value = '222'
await elements.aiTeamA.listeners.change()
await elements.aiTeamSave.listeners.click()
const saved = calls.find(call => call.method === 'PUT')
assert.equal(saved.path, '/v1/local-saves/22/ai-teams')
assert.equal(JSON.stringify(saved.body.party_ids), '[222,221]')
await elements.aiTeamFollow.listeners.click()
assert.equal(selectedSource, null)
assert.equal(elements.aiTeamSlot.value, '11')
assert.match(elements.aiTeamSource.textContent, /跟随当前游戏存档/)

// //// 候选请求乱序时保持当前选择且加载期间锁定保存 [@x380kkm 2026-09-29] ////
delaySlot = 22
const delayedRequest = new Promise(resolve => { notifyDelayedRequest = resolve })
elements.aiTeamSlot.value = '22'
const oldLoad = elements.aiTeamSlot.listeners.change()
await delayedRequest
assert.equal(elements.aiTeamSave.disabled, true)
elements.aiTeamSlot.value = '11'
await elements.aiTeamSlot.listeners.change()
delayedResponse()
await oldLoad
assert.equal(elements.aiTeamA.value, '111')
assert.equal(elements.aiTeamB.value, '112')
assert.equal(elements.aiTeamSave.disabled, false)
assert.equal(selectedSource, null)
console.log('AI 来源回显, 所选存档保存, 恢复当前来源与候选切换验证完成.')
