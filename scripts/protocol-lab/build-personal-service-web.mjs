// audience: internal
// # personal-service-web-build
// 此入口使用项目的 TypeScript 编译器生成可直接嵌入 Rust 的 ES2017 页面脚本.
// 生成文件由本入口维护, --check 比较源码与已有分发脚本.

import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const webRoot = path.join(root, 'core/personal-service/web')
const check = process.argv.includes('--check')
const compatibility = await fs.readFile(path.join(webRoot, 'management/browser-compatibility.js'), 'utf8')

// //// 编译页面模块并连接静态依赖 [@x380kkm 2026-09-26] ////
async function bundle(entry, directory) {
    const modules = new Map()
    async function visit(id) {
        if (modules.has(id)) return
        const relative = id.startsWith('/manage/') ? 'management/' + id.slice(8)
            : id.startsWith('/player/') ? 'player/' + id.slice(8) : id
        const filename = path.join(webRoot, relative)
        if (!filename.startsWith(webRoot + path.sep)) throw new Error(`页面模块路径无效: ${id}`)
        const source = await fs.readFile(filename, 'utf8')
        const compiled = ts.transpileModule(source, {
            fileName: filename,
            compilerOptions: { target: ts.ScriptTarget.ES2017, module: ts.ModuleKind.CommonJS, newLine: ts.NewLineKind.LineFeed },
            reportDiagnostics: true,
        })
        if (compiled.diagnostics.some(item => item.category === ts.DiagnosticCategory.Error)) {
            throw new Error(ts.formatDiagnosticsWithColorAndContext(compiled.diagnostics, {
                getCanonicalFileName: value => value, getCurrentDirectory: () => root, getNewLine: () => '\n',
            }))
        }
        const dependencies = Object.fromEntries(ts.preProcessFile(source).importedFiles.map(imported => [
            imported.fileName, path.posix.resolve(path.posix.dirname(id), imported.fileName),
        ]))
        modules.set(id, { source: compiled.outputText, dependencies })
        for (const dependency of Object.values(dependencies)) {
            await visit(dependency)
        }
    }
    await visit(entry)
    const factories = [...modules].map(([id, module]) => `${JSON.stringify(id)}: function(module, exports, require) {\n${module.source}}`).join(',\n')
    const links = JSON.stringify(Object.fromEntries([...modules].map(([id, module]) => [id, module.dependencies])))
    const contents = `// audience: internal
// # personal-service-browser-bundle
// 此文件由 build-personal-service-web.mjs 从页面源码生成.
${compatibility}
(function () {
"use strict";
const modules = {
${factories}
};
const links = ${links};
const cache = Object.create(null);
function load(id) {
    if (cache[id]) return cache[id].exports;
    const module = { exports: {} };
    cache[id] = module;
    modules[id](module, module.exports, function (dependency) {
        return load(links[id][dependency]);
    });
    return module.exports;
}
load(${JSON.stringify(entry)});
})();
`
    const output = path.join(webRoot, directory, 'browser-app.js')
    if (check) {
        if (await fs.readFile(output, 'utf8') !== contents) throw new Error(`页面分发脚本与源码不一致: ${output}`)
    } else {
        await fs.writeFile(output, contents, 'utf8')
    }
    console.log(`${directory}: ${modules.size} 个模块, ES2017, ${check ? '源码一致' : '生成完成'}`)
}

await bundle('/manage/app.js', 'management')
await bundle('/player/app.js', 'player')
