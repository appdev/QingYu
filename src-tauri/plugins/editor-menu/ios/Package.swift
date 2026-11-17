// swift-tools-version:5.3
import PackageDescription

let package = Package(
    name: "tauri-plugin-editor-menu",
    platforms: [.iOS("15.0")],
    products: [.library(name: "tauri-plugin-editor-menu", type: .static, targets: ["tauri-plugin-editor-menu"])],
    dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")],
    targets: [.target(name: "tauri-plugin-editor-menu", dependencies: [.byName(name: "Tauri")], path: "Sources")]
)
