// audience: internal
// # cn-mana-board-cache-tests
// 使用客户端对象布局和原生边界替身验证玛纳板范围变化, 缓存复用及 AIR 写屏障.

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../PersonalServiceBootstrap/StarpointCNManaBoardCacheHook.m"

typedef struct {
    void *nativeVtable;
    void *delegate;
    void *vtable;
    uint32_t count;
} TestArray;

static uint8_t character[0xa8], characterVtable[0x318];
static uint8_t nodesLazy[0x28], abilitiesLazy[0x28];
static uint8_t nodesEnvironment[0x20], abilitiesEnvironment[0x20], idsEnvironment[0x20];
static uint8_t nodesInfo[0x58], abilitiesInfo[0x58], idsInfo[0x58];
static uint8_t pool[0x10], core[0xc0], scope[0x18], abc[0x38];
static uint8_t scripts[0x298], script[0x28], optionClass[0x40];
static uint8_t arrayVtable[0xc0], lengthEnvironment[0x18], lengthInfo[0x58];
static uint8_t none, cached, collector;
static TestArray nodeArray, abilityArray, idArray;
static unsigned nodeBuilds, abilityBuilds, writes, scriptLoads;
static uint32_t visibleNodes;
static void *priorFrame = (void *)0x1230;

// //// 为 AIR 虚表和写屏障提供可观察的边界 [@x380kkm 2026-09-24] ////
uintptr_t *starpointCnValidatedMethodTable(const StarpointCnMethodSignature *signatures, size_t count) {
    (void)signatures;
    (void)count;
    return NULL;
}

static void putPointer(void *object, size_t offset, void *value) {
    memcpy((uint8_t *)object + offset, &value, sizeof(value));
}

static uint32_t lengthOf(void *receiver, void *environment) {
    assert(environment == lengthEnvironment);
    return ((TestArray *)receiver)->count;
}

static void *currentIds(void *receiver, void *environment) {
    assert(receiver == character && environment == idsEnvironment);
    idArray.count = visibleNodes;
    return &idArray;
}

static void *cachedNodes(void *receiver, void *environment) {
    assert(receiver == character && environment == nodesEnvironment);
    if (readPointer(nodesLazy, lazyValueOffset) == &none) {
        nodeBuilds += 1;
        nodeArray.count = visibleNodes;
        putPointer(nodesLazy, lazyValueOffset, &cached);
    }
    return &nodeArray;
}

static void *cachedAbilities(void *receiver, void *environment) {
    assert(receiver == character && environment == abilitiesEnvironment);
    if (readPointer(abilitiesLazy, lazyValueOffset) == &none) {
        abilityBuilds += 1;
        TestArray *nodes = maxManaNodes(receiver, nodesEnvironment);
        abilityArray.count = nodes->count == 30 ? 3 : 6;
        putPointer(abilitiesLazy, lazyValueOffset, &cached);
    }
    return &abilityArray;
}

static void storePointer(void *gc, void *owner, void **slot, void *value) {
    assert(gc == &collector && value == &none);
    assert(owner == nodesLazy || owner == abilitiesLazy);
    assert(slot == (void **)((uint8_t *)owner + lazyValueOffset));
    AvmMethodFrame *frame = readPointer(core, coreFrameOffset);
    assert(frame->environment == nodesEnvironment && frame->previous == priorFrame);
    writes += 1;
    *slot = value;
}

static void *loadScript(void **slot, AvmMethodFrame *frame) {
    assert(slot == (void **)(scripts + optionScriptSlot));
    assert(frame->environment == nodesEnvironment);
    scriptLoads += 1;
    *slot = script;
    return script;
}

static void interrupt(void *value) { assert(value == core); }
static void nullError(void *environment) { (void)environment; abort(); }
// //// /为 AIR 虚表和写屏障提供可观察的边界 ////

// //// 连接原生方法环境与角色缓存对象 [@x380kkm 2026-09-24] ////
static void prepare(void) {
    visibleNodes = 30;
    nodeBuilds = abilityBuilds = writes = scriptLoads = 0;
    originalMaxManaNodes = cachedNodes;
    originalMaxAbilities = cachedAbilities;
    initializeScript = loadScript;
    writeObjectPointer = storePointer;
    checkInterrupt = interrupt;
    raiseNullObjectError = nullError;
    putPointer(character, objectVtableOffset, characterVtable);
    putPointer(character, maxManaNodesOffset, nodesLazy);
    putPointer(character, maxAbilitiesOffset, abilitiesLazy);
    putPointer(characterVtable, maxManaNodesMethodSlot, nodesEnvironment);
    putPointer(characterVtable, manaNodeIdsMethodSlot, idsEnvironment);
    putPointer(nodesLazy, lazyValueOffset, &none);
    putPointer(abilitiesLazy, lazyValueOffset, &none);
    putPointer(nodesEnvironment, methodInfoOffset, nodesInfo);
    putPointer(abilitiesEnvironment, methodInfoOffset, abilitiesInfo);
    putPointer(idsEnvironment, methodInfoOffset, idsInfo);
    putPointer(nodesInfo, methodPoolOffset, pool);
    putPointer(abilitiesInfo, methodPoolOffset, pool);
    putPointer(idsInfo, methodImplementationOffset, (void *)currentIds);
    putPointer(pool, poolCoreOffset, core);
    putPointer(core, coreFrameOffset, priorFrame);
    putPointer(core, coreCollectorOffset, &collector);
    putPointer(nodesEnvironment, methodScopeOffset, scope);
    putPointer(scope, scopeAbcEnvironmentOffset, abc);
    putPointer(abc, abcScriptEnvironmentsOffset, scripts);
    putPointer(scripts, optionScriptSlot, script);
    putPointer(script, scriptGlobalOffset, optionClass);
    putPointer(optionClass, optionNoneOffset, &none);
    putPointer(arrayVtable, arrayLengthMethodSlot, lengthEnvironment);
    putPointer(lengthEnvironment, methodInfoOffset, lengthInfo);
    putPointer(lengthInfo, methodImplementationOffset, (void *)lengthOf);
    nodeArray.vtable = abilityArray.vtable = idArray.vtable = arrayVtable;
}
// //// /连接原生方法环境与角色缓存对象 ////

// //// 验证开板, 回拨和稳定读取共享同一缓存范围 [@x380kkm 2026-09-24] ////
int main(void) {
    prepare();
    assert(((TestArray *)maxAbilities(character, abilitiesEnvironment))->count == 3);
    assert(nodeBuilds == 1 && abilityBuilds == 1 && writes == 0);
    visibleNodes = 51;
    putPointer(scripts, optionScriptSlot, NULL);
    assert(((TestArray *)maxAbilities(character, abilitiesEnvironment))->count == 6);
    assert(nodeBuilds == 2 && abilityBuilds == 2 && writes == 2 && scriptLoads == 1);
    assert(maxAbilities(character, abilitiesEnvironment) == &abilityArray);
    assert(maxManaNodes(character, nodesEnvironment) == &nodeArray);
    assert(nodeBuilds == 2 && abilityBuilds == 2 && writes == 2);
    visibleNodes = 30;
    assert(((TestArray *)maxAbilities(character, abilitiesEnvironment))->count == 3);
    assert(nodeBuilds == 3 && abilityBuilds == 3 && writes == 4);
    visibleNodes = 51;
    assert(((TestArray *)maxManaNodes(character, nodesEnvironment))->count == 51);
    assert(readPointer(abilitiesLazy, lazyValueOffset) == &none);
    assert(((TestArray *)maxAbilities(character, abilitiesEnvironment))->count == 6);
    assert(nodeBuilds == 4 && abilityBuilds == 4 && writes == 6);
    assert(readPointer(core, coreFrameOffset) == priorFrame);
    prepare();
    visibleNodes = 51;
    assert(((TestArray *)maxAbilities(character, abilitiesEnvironment))->count == 6);
    assert(nodeBuilds == 1 && abilityBuilds == 1 && writes == 0);
    assert(readPointer(core, coreFrameOffset) == priorFrame);
    puts("玛纳板范围变化同步刷新节点与能力, 稳定读取复用缓存, AIR 写屏障和调用帧保持一致.");
    return 0;
}
// //// /验证开板, 回拨和稳定读取共享同一缓存范围 ////
