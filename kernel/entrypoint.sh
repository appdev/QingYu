#!/bin/sh
set -e

# 默认值
PUID=${PUID:-1000}
PGID=${PGID:-1000}
USER_NAME=${USER_NAME:-qingyu}
GROUP_NAME=${GROUP_NAME:-qingyu}
WORKSPACE_DIR=${QINGYU_WORKSPACE_PATH:-/qingyu/workspace}

if [ "$#" -eq 0 ] || [ "$1" != "/opt/qingyu/QingYu-Kernel" ]; then
    echo "The command must start with /opt/qingyu/QingYu-Kernel" >&2
    exit 64
fi

for arg in "$@"; do
    case "$arg" in
        --workspace=*) WORKSPACE_DIR=${arg#*=} ;;
    esac
done
export QINGYU_WORKSPACE_PATH="${WORKSPACE_DIR}"

# 获取或创建用户组
group_name="${GROUP_NAME}"
if getent group "${PGID}" > /dev/null 2>&1; then
    group_name=$(getent group "${PGID}" | cut -d: -f1)
    echo "Using existing group: ${group_name} (${PGID})"
else
    echo "Creating group ${group_name} (${PGID})"
    addgroup --gid "${PGID}" "${group_name}"
fi

# 获取或创建用户
user_name="${USER_NAME}"
if getent passwd "${PUID}" > /dev/null 2>&1; then
    user_name=$(getent passwd "${PUID}" | cut -d: -f1)
    echo "Using existing user ${user_name} (PUID: ${PUID}, PGID: ${PGID})"
else
    echo "Creating user ${user_name} (PUID: ${PUID}, PGID: ${PGID})"
    adduser --uid "${PUID}" --ingroup "${group_name}" --disabled-password --gecos "" "${user_name}"
fi

# 准备轻语运行目录和工作空间权限
mkdir -p /home/qingyu "${WORKSPACE_DIR}"
echo "Adjusting ownership of /opt/qingyu, /home/qingyu/, and ${WORKSPACE_DIR}"
chown -R "${PUID}:${PGID}" /opt/qingyu /home/qingyu/ "${WORKSPACE_DIR}"

# 切换到目标用户并原样执行完整命令
echo "Starting QingYu with UID:${PUID} and GID:${PGID} in workspace ${WORKSPACE_DIR}"
exec su-exec "${PUID}:${PGID}" "$@"
