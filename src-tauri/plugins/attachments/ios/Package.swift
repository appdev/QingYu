// swift-tools-version:5.3
import PackageDescription
let package = Package(name: "tauri-plugin-attachments", platforms: [.iOS("15.0")], products: [.library(name: "tauri-plugin-attachments", type: .static, targets: ["tauri-plugin-attachments"])], dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")], targets: [.target(name: "tauri-plugin-attachments", dependencies: [.byName(name: "Tauri")], path: "Sources")])
