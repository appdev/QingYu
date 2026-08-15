#!/bin/bash

set -u

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
readonly SOURCE_APP="$SCRIPT_DIR/QingYu.app"
readonly TARGET_APP="/Applications/QingYu.app"

show_message() {
    if [ "${QINGYU_INSTALLER_NO_DIALOG:-}" = "1" ]; then
        return
    fi
    /usr/bin/osascript - "$1" <<'APPLESCRIPT' >/dev/null 2>&1 || true
on run argv
    display dialog (item 1 of argv) buttons {"好"} default button "好" with title "QingYu 自动安装"
end run
APPLESCRIPT
}

fail() {
    printf 'QingYu 自动安装失败：%s\n' "$1" >&2
    show_message "安装失败：$1"
    exit 1
}

validate_app() {
    local app_path="$1"
    local plist_path="$app_path/Contents/Info.plist"
    local bundle_id
    local executable_name
    local executable_path
    [ -f "$plist_path" ] || return 1
    bundle_id="$(/usr/bin/plutil -extract CFBundleIdentifier raw -o - "$plist_path" 2>/dev/null)" || return 1
    [ "$bundle_id" = "com.apkdv.qingyu" ] || return 1
    executable_name="$(/usr/bin/plutil -extract CFBundleExecutable raw -o - "$plist_path" 2>/dev/null)" || return 1
    [ -n "$executable_name" ] || return 1
    [ "$executable_name" = "$(/usr/bin/basename "$executable_name")" ] || return 1
    executable_path="$app_path/Contents/MacOS/$executable_name"
    [ -f "$executable_path" ] && [ -x "$executable_path" ] || return 1
}

is_qingyu_running() {
    /usr/bin/pgrep -x "QingYu" >/dev/null 2>&1
}

main() {
    [ "$(/usr/bin/uname -s)" = "Darwin" ] || fail "此脚本只能在 macOS 上运行。"
    [ -d "$SOURCE_APP" ] || fail "未找到与脚本同目录的 QingYu.app。"
    validate_app "$SOURCE_APP" || fail "QingYu.app 结构或应用标识无效。"
    [ "$TARGET_APP" = "/Applications/QingYu.app" ] || fail "安装目标无效。"

    if is_qingyu_running; then
        /usr/bin/osascript -e 'tell application id "com.apkdv.qingyu" to quit' >/dev/null 2>&1 || true
        wait_count=0
        while is_qingyu_running && [ "$wait_count" -lt 10 ]; do
            /bin/sleep 1
            wait_count=$((wait_count + 1))
        done
        is_qingyu_running && fail "QingYu 仍在运行，请手动退出后重试。"
    fi

    if ! /usr/bin/osascript - "$SOURCE_APP" "$TARGET_APP" >/dev/null 2>&1 <<'APPLESCRIPT'
on run argv
    set sourceApp to item 1 of argv
    set targetApp to item 2 of argv
    set commandText to "if /usr/bin/pgrep -x QingYu >/dev/null 2>&1; then exit 73; fi && /bin/rm -rf -- " & quoted form of targetApp & " && /usr/bin/ditto --noqtn " & quoted form of sourceApp & " " & quoted form of targetApp & " && /usr/bin/xattr -cr " & quoted form of targetApp
    do shell script commandText with administrator privileges
end run
APPLESCRIPT
    then
        fail "管理员授权已取消、QingYu 仍在运行，或复制应用时发生错误。"
    fi

    validate_app "$TARGET_APP" || fail "复制完成后的 QingYu.app 验证失败。"
    /usr/bin/open "$TARGET_APP" || fail "应用已安装，但无法自动启动。"
    wait_count=0
    while ! is_qingyu_running && [ "$wait_count" -lt 10 ]; do
        /bin/sleep 1
        wait_count=$((wait_count + 1))
    done
    is_qingyu_running || fail "应用已安装，但未能确认 QingYu 已启动。"
    printf 'QingYu 已安装到 %s。\n' "$TARGET_APP"
    show_message "安装完成，QingYu 已复制到“应用程序”并启动。"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
