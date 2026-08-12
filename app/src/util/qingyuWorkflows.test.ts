import * as assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import {join} from "node:path";
import test from "node:test";
import {parse} from "yaml";

interface IWorkflowTrigger {
    inputs?: Record<string, {
        default?: boolean;
        required?: boolean;
        type?: string;
    }>;
}

interface IWorkflow {
    on: Record<string, IWorkflowTrigger | null>;
}

const repositoryRoot = join(__dirname, "../../..");
const readWorkflow = async (name: string) => {
    const source = await readFile(join(repositoryRoot, `.github/workflows/${name}`), "utf8");
    return {source, workflow: parse(source) as IWorkflow};
};

const assertPrereleaseInput = (trigger: IWorkflowTrigger | null, defaultValue?: boolean) => {
    const input = trigger?.inputs?.prerelease;
    assert.equal(input?.type, "boolean");
    assert.equal(input?.required, true);
    if (defaultValue !== undefined) {
        assert.equal(input?.default, defaultValue);
    }
};

test("CD is manual, reusable, and publishes the requested release type", async () => {
    const {source, workflow} = await readWorkflow("cd.yml");
    assert.deepEqual(Object.keys(workflow.on).sort(), ["workflow_call", "workflow_dispatch"]);
    assertPrereleaseInput(workflow.on.workflow_dispatch, true);
    assertPrereleaseInput(workflow.on.workflow_call);
    assert.match(source, /prerelease: \$\{\{ inputs\.prerelease \}\}/);
    assert.match(source, /RELEASE_TAG="v\$VERSION"/);
    assert.match(source, /\^\[0-9\]\+\\\.\[0-9\]\+\\\.\[0-9\]\+\$/);
    assert.match(source, /-\(alpha\|beta\|rc\)/);
});

test("Docker builds prereleases without publishing and pushes stable images", async () => {
    const {source, workflow} = await readWorkflow("dockerimage.yml");
    assert.deepEqual(Object.keys(workflow.on).sort(), ["workflow_call", "workflow_dispatch"]);
    assertPrereleaseInput(workflow.on.workflow_dispatch, true);
    assertPrereleaseInput(workflow.on.workflow_call);
    assert.match(source, /docker_hub_owner: "apkdv"/);
    assert.match(source, /docker_hub_repo: "qingyu"/);
    assert.match(source, /if: \$\{\{ !inputs\.prerelease \}\}/);
    assert.match(source, /push: \$\{\{ !inputs\.prerelease \}\}/);
    assert.match(source, /apkdv\/qingyu:latest/);
    assert.match(source, /apkdv\/qingyu:v\$\{\{ steps\.version\.outputs\.value \}\}/);
    assert.doesNotMatch(source, /b3log\/siyuan|github\.repository_owner == 'siyuan-note'/);
});

test("release dispatcher passes prerelease to both reusable workflows", async () => {
    const {source, workflow} = await readWorkflow("release.yml");
    assert.deepEqual(Object.keys(workflow.on), ["workflow_dispatch"]);
    assertPrereleaseInput(workflow.on.workflow_dispatch, true);
    assert.match(source, /permissions:\n {2}contents: write/);
    assert.match(source, /uses: \.\/\.github\/workflows\/cd\.yml/);
    assert.match(source, /uses: \.\/\.github\/workflows\/dockerimage\.yml/);
    assert.equal(source.match(/prerelease: \$\{\{ inputs\.prerelease \}\}/g)?.length, 2);
    assert.equal(source.match(/secrets: inherit/g)?.length, 2);
});

test("every workflow has no automatic event trigger", async () => {
    for (const name of ["cd.yml", "dockerimage.yml", "release.yml", "lock.yml"]) {
        const {workflow} = await readWorkflow(name);
        const triggerNames = Object.keys(workflow.on);
        assert.ok(triggerNames.includes("workflow_dispatch"), name);
        assert.deepEqual(
            triggerNames.filter((trigger) => trigger !== "workflow_dispatch" && trigger !== "workflow_call"),
            [],
            name,
        );
    }
});
