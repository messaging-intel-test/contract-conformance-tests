enum MobileHostingMode {
  clientOnly,
  persistentOrigin,
  wakeAndDrain,
}

class KeepalivePromptPolicy {
  const KeepalivePromptPolicy({
    required this.promptsPerDay,
    this.enabled = true,
  });

  final int promptsPerDay;
  final bool enabled;

  void validate() {
    if (promptsPerDay < 3 || promptsPerDay > 5) {
      throw ArgumentError.value(
        promptsPerDay,
        'promptsPerDay',
        'must be between 3 and 5',
      );
    }
  }
}

class MobileHostCapabilities {
  const MobileHostCapabilities({
    required this.platform,
    required this.supportsPersistentOrigin,
    required this.supportsWakeAndDrain,
    required this.supportsBackgroundPush,
    required this.minimumRepairWakeMinutes,
    this.backgroundTaskRegistered = false,
    this.notificationsEnabled = false,
  });

  final String platform;
  final bool supportsPersistentOrigin;
  final bool supportsWakeAndDrain;
  final bool supportsBackgroundPush;
  final int? minimumRepairWakeMinutes;
  final bool backgroundTaskRegistered;
  final bool notificationsEnabled;

  factory MobileHostCapabilities.fromMap(Map<Object?, Object?> map) {
    return MobileHostCapabilities(
      platform: map['platform'] as String? ?? 'unknown',
      supportsPersistentOrigin:
          map['supports_persistent_origin'] as bool? ?? false,
      supportsWakeAndDrain: map['supports_wake_and_drain'] as bool? ?? false,
      supportsBackgroundPush:
          map['supports_background_push'] as bool? ?? false,
      minimumRepairWakeMinutes:
          map['minimum_repair_wake_minutes'] as int?,
      backgroundTaskRegistered:
          map['background_task_registered'] as bool? ?? false,
      notificationsEnabled: map['notifications_enabled'] as bool? ?? false,
    );
  }
}

class MobileHostConfig {
  const MobileHostConfig({
    required this.productId,
    required this.deviceId,
    required this.originPort,
    required this.mode,
    required this.relayUrl,
  });

  final String productId;
  final String deviceId;
  final int originPort;
  final MobileHostingMode mode;
  final Uri relayUrl;

  void validate() {
    if (productId.trim().isEmpty) {
      throw ArgumentError.value(productId, 'productId', 'must not be empty');
    }

    if (deviceId.trim().isEmpty) {
      throw ArgumentError.value(deviceId, 'deviceId', 'must not be empty');
    }

    if (originPort <= 0 || originPort > 65535) {
      throw ArgumentError.value(
        originPort,
        'originPort',
        'must be a valid TCP port',
      );
    }

    if (relayUrl.scheme.toLowerCase() != 'https' || relayUrl.host.isEmpty) {
      throw ArgumentError.value(
        relayUrl,
        'relayUrl',
        'must be an absolute HTTPS URI',
      );
    }

    if (relayUrl.userInfo.isNotEmpty ||
        relayUrl.hasQuery ||
        relayUrl.hasFragment) {
      throw ArgumentError.value(
        relayUrl,
        'relayUrl',
        'must not contain credentials, query parameters, or fragments',
      );
    }
  }
}

class RelayRequestEnvelope {
  const RelayRequestEnvelope({
    required this.requestId,
    required this.method,
    required this.path,
    required this.headers,
    required this.body,
    required this.deadline,
  });

  final String requestId;
  final String method;
  final String path;
  final Map<String, String> headers;
  final List<int> body;
  final DateTime deadline;
}
