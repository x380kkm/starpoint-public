// audience: internal
// # management-save-controls
// 存档表单使用安全 DOM 文本节点, 提交期间锁定同一表单的全部字段.

// //// 创建存档表单控件 [@x380kkm 2026-09-26] ////
export function node(document, tag, className, text) {
    const element = document.createElement(tag)
    if (className) element.className = className
    if (text !== undefined) element.textContent = text
    return element
}

export function field(document, container, name, label, options = {}) {
    const wrapper = node(document, "label", "grow")
    wrapper.append(node(document, "span", "", label))
    const input = node(document, options.choices ? "select" : "input")
    input.name = name
    if (options.choices) {
        for (const [value, text] of options.choices) {
            const option = node(document, "option", "", text)
            option.value = value
            input.append(option)
        }
    } else input.type = options.type || "text"
    for (const key of ["min", "max", "step", "pattern", "placeholder", "maxLength", "autocomplete"]) {
        if (options[key] !== undefined) input[key] = options[key]
    }
    input.required = Boolean(options.required)
    input.value = String(options.value === undefined ? "" : options.value)
    wrapper.append(input)
    if (options.type === "checkbox") {
        wrapper.className = "check-row"
        wrapper.replaceChildren(input, node(document, "span", "", label))
    }
    container.append(wrapper)
    return input
}

export function actionButton(document, label, action, runAction, style = "ghost") {
    const button = node(document, "button", `button ${style}`, label)
    button.type = "button"
    button.addEventListener("click", () => runAction(button, action))
    return button
}

export function submitForm(document, form, fields, label, action, runAction) {
    const button = node(document, "button", "button primary", label)
    button.type = "submit"
    form.append(button)
    let busy = false
    form.addEventListener("submit", (event) => {
        event.preventDefault()
        if (busy) return
        return runAction(button, async () => {
            busy = true
            fields.disabled = true
            try { return await action() }
            finally { fields.disabled = false; busy = false }
        })
    })
    return button
}
// //// /创建存档表单控件 ////
