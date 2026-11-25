import Tauri
import UIKit
import WebKit

private struct TableMenuArgs: Decodable {
    struct Item: Decodable {
        let text: String
        let symbol: String
        let destructive: Bool
    }

    let frame: [CGFloat]
    let sections: [[Item]]
}

/// 工具栏按钮的下拉菜单。每项带一个前端分配的编号，选中后原样带回。
private struct NativeMenuArgs: Decodable {
    struct Item: Decodable {
        let id: Int
        let text: String
        let enabled: Bool
        let destructive: Bool
        let checked: Bool?
        let children: [[Item]]?
    }

    let frame: [CGFloat]
    let sections: [[Item]]
}

private final class MenuButton: UIButton {
    var opened = false
    var choice: Int?
    var didClose: (() -> Void)?

    override func contextMenuInteraction(_ interaction: UIContextMenuInteraction, willDisplayMenuFor configuration: UIContextMenuConfiguration,
                                         animator: (any UIContextMenuInteractionAnimating)?) {
        super.contextMenuInteraction(interaction, willDisplayMenuFor: configuration, animator: animator)
        opened = true
    }

    override func contextMenuInteraction(_ interaction: UIContextMenuInteraction, willEndFor configuration: UIContextMenuConfiguration,
                                         animator: (any UIContextMenuInteractionAnimating)?) {
        super.contextMenuInteraction(interaction, willEndFor: configuration, animator: animator)
        if let animator = animator { animator.addCompletion { [weak self] in self?.didClose?() } }
        else { DispatchQueue.main.async { [weak self] in self?.didClose?() } }
    }
}

private final class EditorMenuPlugin: Plugin {
    private weak var webview: WKWebView?
    private var active: MenuButton?

    override func load(webview: WKWebView) { self.webview = webview }

    @objc func showTableHandleMenu(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(TableMenuArgs.self), args.frame.count == 4 else {
            invoke.reject("表格菜单参数无效")
            return
        }
        DispatchQueue.main.async {
            self.present(frame: args.frame, invoke: invoke, unavailable: "系统表格菜单", build: { button in
                var index = 0
                let groups = args.sections.filter { !$0.isEmpty }.map { section in
                    UIMenu(options: .displayInline, children: section.map { item in
                        let choice = index
                        index += 1
                        return UIAction(title: item.text, image: UIImage(systemName: item.symbol),
                                        attributes: item.destructive ? .destructive : []) { [weak button] _ in
                            button?.choice = choice
                        }
                    })
                }
                return groups.isEmpty ? nil : UIMenu(children: groups)
            }, resolve: { invoke.resolve($0) })
        }
    }

    @objc func showMenu(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(NativeMenuArgs.self), args.frame.count == 4 else {
            invoke.reject("菜单参数无效")
            return
        }
        DispatchQueue.main.async {
            self.present(frame: args.frame, invoke: invoke, unavailable: "系统菜单", build: { button in
                let groups = Self.groups(args.sections, button)
                return groups.isEmpty ? nil : UIMenu(children: groups)
            }, resolve: { choice in
                if let choice = choice { invoke.resolve(["id": choice]) } else { invoke.resolve([:]) }
            })
        }
    }

    private static func groups(_ sections: [[NativeMenuArgs.Item]], _ button: MenuButton) -> [UIMenuElement] {
        sections.filter { !$0.isEmpty }.map { section in
            UIMenu(options: .displayInline, children: section.map { item -> UIMenuElement in
                // UIMenu 不能禁用，不可用的子菜单显示成一行灰色的普通项。
                if let children = item.children, item.enabled {
                    return UIMenu(title: item.text, children: groups(children, button))
                }
                var attributes: UIMenuElement.Attributes = []
                if item.destructive { attributes.insert(.destructive) }
                if !item.enabled { attributes.insert(.disabled) }
                let id = item.id
                return UIAction(title: item.text, attributes: attributes, state: item.checked == true ? .on : .off) { [weak button] _ in
                    button?.choice = id
                }
            })
        }
    }

    /// 在网页坐标 frame 处放一个透明按钮，用它弹出系统菜单；菜单关闭后移除按钮并把选中项交回前端。
    private func present(frame rect: [CGFloat], invoke: Invoke, unavailable name: String,
                         build: (MenuButton) -> UIMenu?, resolve: @escaping (Int?) -> Void) {
        guard #available(iOS 17.4, *), let webview = webview, webview.window != nil else {
            invoke.reject("\(name)不可用")
            return
        }
        guard active == nil else { resolve(nil); return }
        let frame = CGRect(x: rect[0], y: rect[1], width: rect[2], height: rect[3]).intersection(webview.bounds)
        guard !frame.isEmpty else { invoke.reject("菜单按钮不在当前画面内"); return }

        let button = MenuButton(type: .custom)
        button.frame = frame
        button.isAccessibilityElement = false
        button.isUserInteractionEnabled = false
        button.showsMenuAsPrimaryAction = true
        guard let menu = build(button) else { resolve(nil); return }
        button.menu = menu
        button.didClose = { [weak self, weak button] in
            guard let button = button, self?.active === button else { return }
            button.removeFromSuperview()
            self?.active = nil
            resolve(button.choice)
        }
        active = button
        webview.addSubview(button)
        DispatchQueue.main.async { [weak button] in button?.performPrimaryAction() }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self, weak button] in
            guard let button = button, self?.active === button, !button.opened else { return }
            button.removeFromSuperview()
            self?.active = nil
            invoke.reject("\(name)未打开")
        }
    }
}

@_cdecl("init_plugin_editor_menu")
public func initEditorMenuPlugin() -> Plugin { EditorMenuPlugin() }
