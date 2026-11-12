import UIKit
import Tauri

private struct OpenArgs: Decodable { let path: String; let mime: String }
class AttachmentsPlugin: Plugin, UIDocumentInteractionControllerDelegate {
    private var document: UIDocumentInteractionController?
    private weak var presenter: UIViewController?
    @objc func openFile(_ invoke: Invoke) {
        do {
            let args = try invoke.parseArgs(OpenArgs.self)
            let url = URL(fileURLWithPath: args.path).resolvingSymlinksInPath()
            let cache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("attachments").path + "/"
            guard url.path.hasPrefix(cache), FileManager.default.isReadableFile(atPath: url.path) else { invoke.reject("附件路径无效"); return }
            DispatchQueue.main.async {
                var controller = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap { $0.windows }.first { $0.isKeyWindow }?.rootViewController
                while let presented = controller?.presentedViewController { controller = presented }
                guard let controller = controller else { invoke.reject("当前窗口不可用"); return }
                self.presenter = controller
                let document = UIDocumentInteractionController(url: url)
                self.document = document
                document.delegate = self
                if document.presentPreview(animated: true) || document.presentOpenInMenu(from: controller.view.bounds, in: controller.view, animated: true) { invoke.resolve() }
                else { invoke.reject("没有支持此附件的应用") }
            }
        } catch { invoke.reject("附件参数无效") }
    }
    func documentInteractionControllerViewControllerForPreview(_ controller: UIDocumentInteractionController) -> UIViewController { presenter ?? UIViewController() }
}
@_cdecl("init_plugin_attachments")
public func initPlugin() -> Plugin { AttachmentsPlugin() }
