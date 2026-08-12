// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

const shouldSpawnKernel = ({development, managed, workspaceCount}) =>
    !development || managed || workspaceCount > 0;

module.exports = {
    shouldSpawnKernel,
};
