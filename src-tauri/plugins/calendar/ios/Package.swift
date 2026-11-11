// swift-tools-version:5.3
import PackageDescription

let package = Package(
    name: "tauri-plugin-calendar",
    platforms: [.iOS("15.0")],
    products: [.library(name: "tauri-plugin-calendar", type: .static, targets: ["tauri-plugin-calendar"])],
    dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")],
    targets: [.target(name: "tauri-plugin-calendar", dependencies: [.byName(name: "Tauri")], path: "Sources", linkerSettings: [.linkedFramework("EventKit")])]
)
