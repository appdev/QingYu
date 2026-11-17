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

private final class TableMenuButton: UIButton {
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
    private var active: TableMenuButton?

    override func load(webview: WKWebView) { self.webview = webview }

    @objc func showTableHandleMenu(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(TableMenuArgs.self), args.frame.count == 4 else {
            invoke.reject("表格菜单参数无效")
            return
        }
        DispatchQueue.main.async { self.present(args, invoke) }
    }

    private func present(_ args: TableMenuArgs, _ invoke: Invoke) {
        guard #available(iOS 17.4, *), let webview = webview, webview.window != nil else {
            invoke.reject("系统表格菜单不可用")
            return
        }
        guard active == nil else { invoke.resolve(nil as Int?); return }
        let frame = CGRect(x: args.frame[0], y: args.frame[1], width: args.frame[2], height: args.frame[3]).intersection(webview.bounds)
        guard !frame.isEmpty else { invoke.reject("表格手柄不在当前画面内"); return }

        let button = TableMenuButton(type: .custom)
        button.frame = frame
        button.isAccessibilityElement = false
        button.isUserInteractionEnabled = false
        button.showsMenuAsPrimaryAction = true
        var index = 0
        let groups = args.sections.filter { !$0.isEmpty }.map { section in
            UIMenu(options: .displayInline, children: section.map { item in
                let choice = index
                index += 1
                return UIAction(title: item.text, image: UIImage(systemName: item.symbol), attributes: item.destructive ? .destructive : []) { [weak button] _ in
                    button?.choice = choice
                }
            })
        }
        guard !groups.isEmpty else { invoke.resolve(nil as Int?); return }
        button.menu = UIMenu(children: groups)
        button.didClose = { [weak self, weak button] in
            guard let button = button, self?.active === button else { return }
            button.removeFromSuperview()
            self?.active = nil
            invoke.resolve(button.choice)
        }
        active = button
        webview.addSubview(button)
        DispatchQueue.main.async { [weak button] in button?.performPrimaryAction() }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self, weak button] in
            guard let button = button, self?.active === button, !button.opened else { return }
            button.removeFromSuperview()
            self?.active = nil
            invoke.reject("系统表格菜单未打开")
        }
    }
}

@_cdecl("init_plugin_editor_menu")
public func initEditorMenuPlugin() -> Plugin { EditorMenuPlugin() }
