// swift-tools-version:5.10
// RustBridge: local SPM package that exposes the generated A0Platform
// UniFFI bindings + the Rust static library (XCFramework) to the app target.
//
// Checked in: Package.swift, README.md
// Generated (by scripts/build-ios.sh, NOT checked in):
//   Sources/A0Platform/A0Platform.swift        (UniFFI 0.31, library mode)
//   Sources/A0Platform/include/*.h, module.modulemap
//   Frameworks/A0Platform.xcframework
import PackageDescription

let package = Package(
    name: "RustBridge",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "RustBridge", targets: ["A0Platform"])
    ],
    targets: [
        .binaryTarget(
            name: "A0PlatformStatic",
            path: "Frameworks/A0Platform.xcframework"
        ),
        .target(
            name: "A0Platform",
            dependencies: ["A0PlatformStatic"],
            path: "Sources/A0Platform",
            cSettings: [
                .unsafeFlags(["-I", "Sources/A0Platform/include"])
            ]
        )
    ]
)
