// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "ores_common_mobile_host",
    platforms: [
        .iOS("13.0"),
    ],
    products: [
        .library(
            name: "ores-common-mobile-host",
            targets: ["ores_common_mobile_host"]
        ),
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework"),
    ],
    targets: [
        .target(
            name: "ores_common_mobile_host",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
            ]
        ),
    ]
)
