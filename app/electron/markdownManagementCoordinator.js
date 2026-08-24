const createMarkdownManagementCoordinator = ({timeout = 5000} = {}) => {
    const renderers = new Map();
    const workspaceEpochs = new Map();
    const pending = new Map();
    const leases = new Map();
    let nextGeneration = 1;

    const workspaceEpoch = (workspace) => workspaceEpochs.get(workspace) || 0;
    const fail = (operationID) => {
        const operation = pending.get(operationID);
        if (!operation) return;
        clearTimeout(operation.timer);
        pending.delete(operationID);
        operation.resolve(operation.phase === "commit" ? {ok: false} : {ok: false, matches: 0});
    };
    const changeWorkspace = (workspace) => {
        workspaceEpochs.set(workspace, workspaceEpoch(workspace) + 1);
        for (const [operationID, operation] of pending) {
            if (operation.workspace === workspace) fail(operationID);
        }
        for (const [operationID, lease] of leases) {
            if (lease.workspace === workspace) leases.delete(operationID);
        }
    };
    const finish = (operationID) => {
        const operation = pending.get(operationID);
        if (!operation || operation.results.size !== operation.expected.size) return;
        clearTimeout(operation.timer);
        pending.delete(operationID);
        if ([...operation.results.values()].some((result) => !result.ok) ||
            workspaceEpoch(operation.workspace) !== operation.epoch) {
            operation.resolve(operation.phase === "commit" ? {ok: false} : {ok: false, matches: 0});
            return;
        }
        if (operation.phase === "commit") {
            operation.resolve({ok: true});
            return;
        }
        const results = [...operation.results.values()];
        const matches = results.reduce((total, result) => total + (result.matches ?? (result.matched ? 1 : 0)), 0);
        if (operation.mode === "presence") {
            operation.resolve({ok: true, matches});
            return;
        }
        const revisions = new Set(results.filter((result) => result.matched).map((result) => result.revision).filter(Boolean));
        if (operation.expectedRevision) revisions.add(operation.expectedRevision);
        if (matches > 0 && revisions.size !== 1) {
            operation.resolve({ok: false, matches});
            return;
        }
        const lease = `${operationID}:${operation.epoch}:${Math.random().toString(36).slice(2)}`;
        leases.set(operationID, {
            lease,
            workspace: operation.workspace,
            epoch: operation.epoch,
            initiator: operation.initiator,
            participants: operation.expected,
        });
        operation.resolve({
            ok: true,
            ...(revisions.size === 1 ? {revision: [...revisions][0]} : {}),
            matches,
            lease,
        });
    };
    const start = (phase, initiator, request, participants) => new Promise((resolve) => {
        const operation = {
            phase,
            workspace: request.workspace,
            epoch: workspaceEpoch(request.workspace),
            initiator,
            mode: request.mode,
            expectedRevision: request.expectedRevision,
            expected: new Map(participants.map(([id, renderer]) => [id, renderer.generation])),
            results: new Map(),
            resolve,
        };
        operation.timer = setTimeout(() => fail(request.operationID), timeout);
        pending.set(request.operationID, operation);
        try {
            participants.forEach(([, renderer]) => renderer.send({...request, phase, generation: renderer.generation}));
        } catch {
            fail(request.operationID);
        }
    });

    return {
        register(id, workspace, send) {
            const existing = renderers.get(id);
            if (existing?.workspace === workspace) {
                existing.send = send;
                return existing.generation;
            }
            if (existing) changeWorkspace(existing.workspace);
            changeWorkspace(workspace);
            const generation = nextGeneration++;
            renderers.set(id, {workspace, send, generation});
            return generation;
        },
        unregister(id) {
            const renderer = renderers.get(id);
            if (!renderer) return;
            renderers.delete(id);
            changeWorkspace(renderer.workspace);
        },
        prepare(initiator, request) {
            if (!request.operationID || pending.has(request.operationID) || leases.has(request.operationID)) {
                return Promise.resolve({ok: false, matches: 0});
            }
            const participants = [...renderers.entries()].filter(([, renderer]) => renderer.workspace === request.workspace);
            if (!participants.some(([id]) => id === initiator) || participants.length === 0) {
                return Promise.resolve({ok: false, matches: 0});
            }
            return start("prepare", initiator, request, participants);
        },
        commit(initiator, request) {
            const lease = leases.get(request.operationID);
            if (!lease || lease.lease !== request.lease || lease.workspace !== request.workspace ||
                lease.initiator !== initiator || lease.epoch !== workspaceEpoch(request.workspace) ||
                pending.has(request.operationID)) {
                return Promise.resolve({ok: false});
            }
            const participants = [...lease.participants.entries()].map(([id, generation]) => [id, generation, renderers.get(id)])
                .filter(([, generation, renderer]) => renderer?.workspace === request.workspace && renderer.generation === generation)
                .map(([id, , renderer]) => [id, renderer]);
            if (participants.length !== lease.participants.size) {
                leases.delete(request.operationID);
                return Promise.resolve({ok: false});
            }
            leases.delete(request.operationID);
            return start("commit", initiator, request, participants);
        },
        abort(initiator, request) {
            const lease = leases.get(request.operationID);
            if (lease?.initiator === initiator && lease.lease === request.lease) leases.delete(request.operationID);
        },
        ack(id, workspace, result) {
            const operation = pending.get(result.operationID);
            const renderer = renderers.get(id);
            if (!operation || !renderer || renderer.workspace !== workspace || operation.workspace !== workspace ||
                operation.phase !== result.phase || operation.expected.get(id) !== result.generation ||
                renderer.generation !== result.generation || result.workspace !== workspace ||
                operation.phase === "prepare" && result.mode !== operation.mode || operation.results.has(id)) return;
            operation.results.set(id, result);
            finish(result.operationID);
        },
    };
};

const shouldUnregisterMarkdownRendererNavigation = (isInPlace, isMainFrame) => isMainFrame && !isInPlace;

module.exports = {createMarkdownManagementCoordinator, shouldUnregisterMarkdownRendererNavigation};
