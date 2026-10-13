// audience: internal
// # personal-service-web-compatibility-tests
// 分发脚本使用 ES2017 语法. 隔离浏览器环境验证存档页面接线与缺少运行时接口时的操作.

import assert from 'node:assert/strict'
import { Blob, File } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import vm from 'node:vm'
import ts from 'typescript'

const require = createRequire(import.meta.url)
const { parse } = require('acorn')
const web = new URL('../../core/personal-service/web/', import.meta.url)
for (const page of ['management', 'player']) {
    parse(await readFile(new URL(`${page}/browser-app.js`, web), 'utf8'), { ecmaVersion: 2017, sourceType: 'script' })
}

const context = vm.createContext({ setTimeout, assert })
vm.runInContext(`
    delete Object.fromEntries; delete Array.prototype.flatMap; delete globalThis.globalThis;
    this.window = this;
    class Node {
        constructor() { this.children = []; this.listeners = {}; this.ownerDocument = document; this._value = ''; }
        appendChild(node) {
            if (node instanceof DocumentFragment) { for (const child of node.children) this.appendChild(child); }
            else { this.children.push(node); node.parentNode = this; }
            return node;
        }
        append(...nodes) { nodes.forEach(node => this.appendChild(node)); }
        get firstChild() { return this.children[0]; }
        removeChild(node) { this.children.splice(this.children.indexOf(node), 1); }
        remove() { this.parentNode.removeChild(this); }
        set value(value) { this._value = value; if (this.tagName === 'select') this.children.forEach(child => { child.selected = child.value === value; }); }
        get value() { return this.tagName === 'select' ? (this.children.find(child => child.selected) || this.children[0] || {})._value || '' : this._value; }
        addEventListener(name, handler) { this.listeners[name] = handler; }
        focus() {}
    }
    class Element extends Node {}
    class DocumentFragment extends Node {}
    this.Node = Node; this.Element = Element; this.DocumentFragment = DocumentFragment;
    this.document = {
        listeners: {},
        createElement(tagName) { const node = new Element(); node.tagName = tagName; return node; },
        createDocumentFragment() { return new DocumentFragment(); },
        createTextNode(text) { const node = new Node(); node.textContent = text; return node; },
        addEventListener(name, handler) { this.listeners[name] = handler; },
    };
    this.Blob = class Blob { constructor(parts) { this.content = parts.join(''); this.size = this.content.length; } };
    this.File = class File extends Blob {};
    this.FileReader = class FileReader {
        readAsText(blob) { if (blob.fail) { this.error = new Error('read failed'); this.onerror(); } else { this.result = blob.content; this.onload(); } }
    };
`, context)
vm.runInContext(await readFile(new URL('management/browser-compatibility.js', web), 'utf8'), context)
vm.runInContext(`
    assert.equal(globalThis, window);
    const entries = Object.fromEntries([['key', 7], ['__proto__', 5]]);
    assert.equal(entries.key, 7); assert.equal(entries.__proto__, 5);
    assert.equal([1, 2].flatMap(value => [value, value + 1]).join(','), '1,2,2,3');
    const parent = document.createElement('div'); parent.append(document.createElement('b'));
    parent.replaceChildren('fresh', document.createElement('a'));
    assert.equal(parent.children.length, 2); assert.equal(parent.children[0].textContent, 'fresh');
    const form = { elements: [] };
    const revoke = { type: 'submit', value: 'revoke', form };
    document.listeners.click({ target: { closest() { return revoke; } } });
    const submit = { target: form }; document.listeners.submit(submit);
    assert.equal(submit.submitter, revoke);
`, context)

async function moduleExports(name, dependencies = {}) {
    const source = await readFile(new URL(`management/${name}.js`, web), 'utf8')
    const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2017, module: ts.ModuleKind.CommonJS } }).outputText
    context.exports = {}; context.require = id => dependencies[id] || {}
    vm.runInContext(compiled, context)
    return context.exports
}
const backup = await moduleExports('password-backup')
context.backup = backup
await vm.runInContext(`(async () => {
    const calls = [];
    const request = async (path, options) => { calls.push({ path, options }); return { id: 9 }; };
    const form = new Map([['file', new File([JSON.stringify({ format: 'starpoint-password-save' })])], ['password', 'backup-password']]);
    await backup.importSaveFile(form, request);
    assert.equal(calls[0].path, '/v1/local-saves/import-password');
    assert.equal(calls[0].options.body.password, 'backup-password');
    form.set('file', new File([JSON.stringify({ format: 'starpoint-encrypted-save' })]));
    form.set('recovery', new File([JSON.stringify({ package: { format: 'starpoint-save-recovery' } })]));
    await backup.importSaveFile(form, request);
    assert.equal(calls[1].path, '/v1/local-saves/import-encrypted-recovery');
    assert.equal(calls[1].options.body.recovery.format, 'starpoint-save-recovery');
    form.delete('recovery'); await backup.importSaveFile(form, request);
    assert.equal(calls[2].path, '/v1/local-saves/import-encrypted');
    const broken = new File(['x']); broken.fail = true;
    await assert.rejects(broken.text(), /read failed/);
    const card = document.createElement('div');
    const answer = backup.requestBackupPassword(card);
    const passwordForm = card.children[0];
    const inputs = passwordForm.children.slice(0, 2).map(label => label.children[1]);
    inputs[0].value = 'backup-password'; inputs[1].value = 'backup-password';
    passwordForm.listeners.submit({ preventDefault() {} });
    assert.equal(await answer, 'backup-password');
    assert.equal(card.children.length, 0); assert.equal(inputs[0].value, '');
})()`, context)
const views = await moduleExports('views', { './password-backup.js': backup })
context.renderDevicePicker = views.renderDevicePicker
context.selectedDeviceSlotId = views.selectedDeviceSlotId
vm.runInContext(`
    const picker = document.createElement('select');
    const model = { saves: { devices: [], slots: [] } };
    renderDevicePicker(model, { deviceSelect: picker });
    assert.equal(picker.value, 'local'); assert.equal(picker.children[0].textContent, '本机');
    model.saves.devices = [{ device_id: 99, active_slot_id: 1 }];
    renderDevicePicker(model, { deviceSelect: picker }); assert.equal(picker.value, 'local');
    picker.value = '99'; renderDevicePicker(model, { deviceSelect: picker }); assert.equal(picker.value, '99');
    model.saves.local_slot_id = 7;
    assert.equal(selectedDeviceSlotId(model.saves, 'local'), 7);
    assert.equal(selectedDeviceSlotId(model.saves, '99'), 1);
`, context)
// //// 回放存档文件分享与下载的能力分支 [@x380kkm 2026-09-26] ////
context.Blob = Blob
context.File = File
await vm.runInContext(`(async () => {
    const filename = '存档.starpoint-save';
    const payload = { format: 'starpoint-password-save', encrypted: 'portable-backup' };
    const contents = JSON.stringify(payload, null, 2);
    for (const variant of [
        { expected: 'download_requested', shares: 0 },
        { canShare: () => false, expected: 'download_requested', shares: 0 },
        { canShare: () => true, expected: 'shared', shares: 1 },
        { canShare: () => true, error: 'AbortError', expected: 'cancelled', shares: 1 },
        { canShare: () => true, error: 'NotAllowedError', expected: 'download_requested', shares: 1 },
    ]) {
        let downloads = 0;
        let shares = 0;
        let downloaded;
        let shared;
        const link = { click() { downloads++; }, remove() {} };
        document.body = { append() {} };
        document.createElement = () => link;
        window.URL = { createObjectURL(blob) { downloaded = blob; return 'blob:save'; }, revokeObjectURL() {} };
        window.setTimeout = callback => callback();
        window.navigator = { canShare: variant.canShare, async share(data) {
            shares++; shared = data;
            if (variant.error) throw Object.assign(new Error('share failed'), { name: variant.error });
        } };
        assert.equal(await shareOrDownloadJson(filename, payload), variant.expected);
        assert.equal(shares, variant.shares);
        assert.equal(downloads, variant.expected === 'download_requested' ? 1 : 0);
        if (downloaded) {
            assert.equal(link.download, filename);
            assert.equal(await downloaded.text(), contents);
        }
        if (shared) {
            assert.equal(shared.files.length, 1);
            assert.equal(shared.files[0].name, filename);
            assert.equal(shared.files[0].type, 'application/json');
            assert.equal(await shared.files[0].text(), contents);
        }
    }
})()`, context)
// //// /回放存档文件分享与下载的能力分支 ////
console.log('管理页与玩家页 ES2017 解析, 旧运行时, 密码迁移, 本机默认选择与文件交付分支验证完成.')

await import('../../core/personal-service/web/management/save-transfer.test.mjs')
