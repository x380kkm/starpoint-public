// audience: internal
// # android-document-transfer-test
//
// 此回放执行 WebView 注入的结果接口及页面下载函数, 检查写入完成, 取消与失败的反馈.

import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import vm from 'node:vm'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const read = relative => readFile(path.join(root, relative), 'utf8')

// //// 回放原生回执与页面下载的异步边界 [@x380kkm 2026-09-26] ////
const nativeSource = await read('platforms/android/CompanionHost/src/dev/starpoint/personalservice/ManagementDocuments.java')
const expression = nativeSource.match(/String script = ([\s\S]*?);\r?\n        webView\.evaluateJavascript/)[1]
const script = vm.runInNewContext(expression.replace('JSONObject.quote(pageToken)', JSON.stringify(JSON.stringify('document-test-token'))))
let request
const native = { exportJson: (...args) => { request = args } }
const nativeContext = vm.createContext({ window: {}, StarpointDocumentBridge: native })
vm.runInContext(script, nativeContext)
const bridge = nativeContext.window.StarpointDocuments
let settled = false
const saved = bridge.saveJson('存档.json', '{"name":"角色"}').then(value => { settled = true; return value })
await Promise.resolve()
assert.equal(settled, false)
bridge.complete('different-page', request[1], 'saved', '')
await Promise.resolve()
assert.equal(settled, false)
bridge.complete(request[0], request[1], 'saved', '')
assert.equal(await saved, 'saved')
const cancelled = bridge.saveJson('save.json', '{}')
bridge.complete(request[0], request[1], 'cancelled', '')
assert.equal(await cancelled, 'cancelled')
const failed = bridge.saveJson('save.json', '{}')
bridge.complete(request[0], request[1], 'error', '写入失败')
await assert.rejects(failed, /写入失败/)

for (const [relative, name, ending] of [
    ['core/personal-service/web/management/views.js', 'shareOrDownloadJson', '\n// ////'],
    ['core/personal-service/web/player/app.js', 'downloadJson', '\n\nfunction safeFilename'],
]) {
    const source = (await read(relative)).replaceAll('\r\n', '\n')
    const start = source.indexOf('async function ' + name + '(')
    assert.ok(start >= 0)
    const functionSource = source.slice(start, source.indexOf(ending, start))
    let resolveWrite
    let captured
    const window = { StarpointDocuments: { saveJson: (filename, contents) => {
        captured = { filename, contents }
        return new Promise(resolve => { resolveWrite = resolve })
    } } }
    const context = vm.createContext({ window })
    vm.runInContext(functionSource, context)
    const result = context[name]('存档.starpoint-save', { name: '角色' })
    assert.equal(captured.filename, '存档.starpoint-save')
    assert.equal(JSON.parse(captured.contents).name, '角色')
    resolveWrite('cancelled')
    assert.equal(await result, 'cancelled')
    let clicked = false
    let revoked = false
    const link = { click() { clicked = true }, remove() {} }
    context.window = {}
    context.Blob = Blob
    context.navigator = {}
    context.URL = { createObjectURL: () => 'blob:document-test', revokeObjectURL: () => { revoked = true } }
    context.document = { createElement: () => link, body: { append() {} } }
    context.setTimeout = callback => callback()
    assert.equal(await context[name]('save.json', { value: 3 }), 'download_requested')
    assert.equal(clicked, true)
    assert.equal(revoked, true)
    assert.equal(link.download, 'save.json')
}
console.log('文档写入回执, 页面隔离, 取消/失败反馈与浏览器下载回放通过.')
