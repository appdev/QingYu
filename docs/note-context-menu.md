# 笔记存储与右键菜单

桌面笔记已迁移到真实 Markdown 文件，文件夹对应磁盘目录；旧 `localStorage` 键只作为首次迁移输入和备份保留，不再承担正文保存。完整文件行为见 [file-library.md](file-library.md)。文件提交成功后才更新内存；失败保留原文与编辑缓冲。

名称、置顶、废纸篓使用可选元数据，兼容既有笔记库。重命名只改名称；正文与修改时间保持原值。副本使用妙言的 ` Copy` / ` Copy 2` 命名，复制前提交编辑区。废纸篓保留正文及原文件夹，支持恢复或移动到指定文件夹；没有永久删除入口。元数据随现有手动 S3 同步一起保存。

菜单以妙言 `9ce84b01bd5f6b23a7156e317b2a34a464e3df76` 为参考：

- `Views/NotesTableView.swift:rightMouseDown`：先选中鼠标所在行。
- `Resources/Localization/Base.lproj/Main.storyboard`：菜单名称、顺序与快捷键。
- `Controllers/ViewController+Action.swift:duplicate/loadMoveMenu`：复制最新内容；移动子菜单包含废纸篓和其他文件夹。
- `Business/Note.swift:getDupeName`：副本命名。

桌面 App 使用 Tauri 系统原生菜单，可在 Finder、Windows 文件资源管理器或 Linux 默认文件管理器中定位笔记原文件。网页提供可键盘操作的菜单，Web 和移动端隐藏文件定位入口。

验证记录在 `artifacts/note-menu/`。不访问真实 S3；手机测试保持暂停。
