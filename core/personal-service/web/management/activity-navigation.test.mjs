// audience: internal
// # personal-service-activity-navigation-tests
// 同名领主的不同入口保持独立, 每个入口的难度按关卡编号排序.

import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"

const source = await readFile(new URL("./activity-navigation.js", import.meta.url), "utf8")
const { activityGroups, activitySection, questStageKey } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`)
const items = [
    {activity_id: "quest:2:1022004", name: "青之女王 超级", kind: "quest"},
    {activity_id: "quest:2:1035004", name: "青之女王 超级", kind: "quest"},
    {activity_id: "quest:2:1022001", name: "青之女王 中级", kind: "quest"},
]
const groups = activityGroups(items)
assert.equal(groups.length, 2)
assert.notEqual(groups[0].name, groups[1].name)
assert.deepEqual(groups.find(group => group.key === "2:1022").items.map(item => item.activity_id), [
    "quest:2:1022001", "quest:2:1022004",
])
assert.equal(questStageKey({category_id:2, quest_id:1022004}), "2:1022")
assert.equal(activitySection(items[0]), "boss")
assert.equal(activitySection({activity_id: "quest:6:13001", kind: "quest"}), "daily")
assert.equal(activitySection({activity_id: "pass-card:1", kind: "pass-card"}), "commerce")
assert.equal(activitySection({activity_id: "world-story:500005", kind: "world-story"}), "events")
