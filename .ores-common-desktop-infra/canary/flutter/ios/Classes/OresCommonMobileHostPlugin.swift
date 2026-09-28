import BackgroundTasks
import Flutter
import UIKit
import UserNotifications

public final class OresCommonMobileHostPlugin: NSObject, FlutterPlugin {
    private static let channelName = "ores_common_mobile_host/background"
    private static let refreshTaskIdentifier = "com.oresoftware.common-mobile-host.refresh"
    private static let keepaliveIdentifierPrefix = "com.oresoftware.common-mobile-host.keepalive."
    private static let minimumRepairWakeSeconds: TimeInterval = 15 * 60
    private static weak var sharedInstance: OresCommonMobileHostPlugin?

    private let channel: FlutterMethodChannel

    private init(channel: FlutterMethodChannel) {
        self.channel = channel
        super.init()

        Self.sharedInstance = self

        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: Self.refreshTaskIdentifier,
            using: nil
        ) { [weak self] task in
            guard let refreshTask = task as? BGAppRefreshTask else {
                task.setTaskCompleted(success: false)
                return
            }

            self?.handleRefreshTask(refreshTask)
        }
    }

    public static func register(with registrar: FlutterPluginRegistrar) {
        let channel = FlutterMethodChannel(
            name: channelName,
            binaryMessenger: registrar.messenger()
        )
        let instance = OresCommonMobileHostPlugin(channel: channel)
        registrar.addMethodCallDelegate(instance, channel: channel)
    }

    public static func handleBackgroundPush(
        completion: @escaping (UIBackgroundFetchResult) -> Void
    ) {
        guard let instance = sharedInstance else {
            completion(.failed)
            return
        }

        instance.channel.invokeMethod(
            "wakeAndDrain",
            arguments: ["reason": "ios_background_push"]
        ) { result in
            if result is FlutterError {
                completion(.failed)
                return
            }

            completion(.newData)
        }
    }

    public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        switch call.method {
        case "capabilities":
            result([
                "platform": "ios",
                "supports_persistent_origin": false,
                "supports_wake_and_drain": true,
                "supports_background_push": true,
                "minimum_repair_wake_minutes": NSNull(),
            ])

        case "startPersistentHosting":
            result(
                FlutterError(
                    code: "persistent_hosting_unsupported",
                    message: "iOS does not guarantee a continuously running background HTTP origin",
                    details: nil
                )
            )

        case "stopPersistentHosting":
            BGTaskScheduler.shared.cancel(
                taskRequestWithIdentifier: Self.refreshTaskIdentifier
            )
            result(nil)

        case "scheduleRepairWake":
            scheduleRepairWake(call, result: result)

        case "configureKeepalivePrompts":
            configureKeepalivePrompts(call, result: result)

        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func scheduleRepairWake(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        let arguments = call.arguments as? [String: Any]
        let requestedSeconds = arguments?["minimum_delay_seconds"] as? NSNumber
        let delay = max(
            Self.minimumRepairWakeSeconds,
            requestedSeconds?.doubleValue ?? Self.minimumRepairWakeSeconds
        )

        let request = BGAppRefreshTaskRequest(
            identifier: Self.refreshTaskIdentifier
        )
        request.earliestBeginDate = Date(timeIntervalSinceNow: delay)

        do {
            try BGTaskScheduler.shared.submit(request)
            result([
                "earliest_begin_seconds": delay,
                "exact": false,
            ])
        } catch {
            result(
                FlutterError(
                    code: "background_task_schedule_failed",
                    message: error.localizedDescription,
                    details: nil
                )
            )
        }
    }

    private func configureKeepalivePrompts(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        let arguments = call.arguments as? [String: Any]
        let enabled = arguments?["enabled"] as? Bool ?? false
        let promptsPerDay = arguments?["prompts_per_day"] as? Int ?? 4

        guard promptsPerDay >= 3 && promptsPerDay <= 5 else {
            result(
                FlutterError(
                    code: "invalid_prompt_frequency",
                    message: "prompts_per_day must be between 3 and 5",
                    details: nil
                )
            )
            return
        }

        let center = UNUserNotificationCenter.current()
        let identifiers = (0..<5).map {
            "\(Self.keepaliveIdentifierPrefix)\($0)"
        }
        center.removePendingNotificationRequests(withIdentifiers: identifiers)

        guard enabled else {
            result(nil)
            return
        }

        center.requestAuthorization(options: [.alert, .sound]) { granted, error in
            if let error = error {
                result(
                    FlutterError(
                        code: "notification_authorization_failed",
                        message: error.localizedDescription,
                        details: nil
                    )
                )
                return
            }

            guard granted else {
                result(
                    FlutterError(
                        code: "notification_authorization_denied",
                        message: "Keepalive prompts require notification permission",
                        details: nil
                    )
                )
                return
            }

            let spanHours = 12
            let denominator = max(1, promptsPerDay - 1)

            for index in 0..<promptsPerDay {
                let hour = 9 + (index * spanHours / denominator)
                var components = DateComponents()
                components.hour = hour

                let content = UNMutableNotificationContent()
                content.title = "Keep local hosting available"
                content.body = "Tap to reopen the app and refresh its local server/tunnel session."
                content.sound = .default

                let trigger = UNCalendarNotificationTrigger(
                    dateMatching: components,
                    repeats: true
                )
                let request = UNNotificationRequest(
                    identifier: "\(Self.keepaliveIdentifierPrefix)\(index)",
                    content: content,
                    trigger: trigger
                )
                center.add(request)
            }

            result([
                "prompts_per_day": promptsPerDay,
                "exact": false,
            ])
        }
    }

    private func handleRefreshTask(_ task: BGAppRefreshTask) {
        scheduleNextRepairWake()

        task.expirationHandler = { [weak self] in
            self?.channel.invokeMethod(
                "cancelWakeAndDrain",
                arguments: ["reason": "ios_background_time_expired"]
            )
        }

        channel.invokeMethod(
            "wakeAndDrain",
            arguments: ["reason": "ios_bg_app_refresh"]
        ) { result in
            if result is FlutterError {
                task.setTaskCompleted(success: false)
                return
            }

            task.setTaskCompleted(success: true)
        }
    }

    private func scheduleNextRepairWake() {
        let request = BGAppRefreshTaskRequest(
            identifier: Self.refreshTaskIdentifier
        )
        request.earliestBeginDate = Date(
            timeIntervalSinceNow: Self.minimumRepairWakeSeconds
        )

        try? BGTaskScheduler.shared.submit(request)
    }
}
