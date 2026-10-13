// audience: internal
// # personal-service-browser-compatibility
// 管理页与玩家页共用 ES2017 运行时. 补充接口保持浏览器已有实现.

// //// 补充页面使用的集合与文档接口 [@x380kkm 2026-09-26] ////
(function (root) {
    function define(target, name, value) {
        if (!target[name]) Object.defineProperty(target, name, { configurable: true, writable: true, value });
    }
    define(root, "globalThis", root);
    define(Object, "fromEntries", function (entries) {
        const result = {};
        for (const entry of entries) {
            Object.defineProperty(result, entry[0], { configurable: true, enumerable: true, writable: true, value: entry[1] });
        }
        return result;
    });
    define(Array.prototype, "flatMap", function (mapper, thisArg) {
        return this.reduce(function (result, value, index, source) {
            return result.concat(mapper.call(thisArg, value, index, source));
        }, []);
    });
    for (const constructor of [root.Element, root.Document, root.DocumentFragment]) {
        if (!constructor) continue;
        define(constructor.prototype, "replaceChildren", function () {
            const fragment = (this.ownerDocument || this).createDocumentFragment();
            for (const value of arguments) {
                fragment.appendChild(value instanceof root.Node ? value : (this.ownerDocument || this).createTextNode(String(value)));
            }
            while (this.firstChild) this.removeChild(this.firstChild);
            this.appendChild(fragment);
        });
    }
    if (root.Blob) define(root.Blob.prototype, "text", function () {
        const blob = this;
        return new Promise(function (resolve, reject) {
            const reader = new root.FileReader();
            reader.onload = function () { resolve(reader.result); };
            reader.onerror = function () { reject(reader.error || new Error("文件读取失败.")); };
            reader.onabort = function () { reject(new Error("文件读取已取消.")); };
            reader.readAsText(blob, "UTF-8");
        });
    });
    if (root.SubmitEvent && "submitter" in root.SubmitEvent.prototype) return;
    const submitters = new WeakMap();
    root.document.addEventListener("click", function (event) {
        const control = event.target.closest && event.target.closest("button, input");
        if (!control || !control.form || !["submit", "image"].includes(control.type)) return;
        submitters.set(control.form, control);
        root.setTimeout(function () {
            if (submitters.get(control.form) === control) submitters.delete(control.form);
        }, 0);
    }, true);
    root.document.addEventListener("submit", function (event) {
        if ("submitter" in event) return;
        const form = event.target;
        const control = submitters.get(form) || Array.from(form.elements).find(function (element) {
            return element.type === "submit" && !element.disabled;
        }) || null;
        Object.defineProperty(event, "submitter", { value: control });
        submitters.delete(form);
    }, true);
})(window);
