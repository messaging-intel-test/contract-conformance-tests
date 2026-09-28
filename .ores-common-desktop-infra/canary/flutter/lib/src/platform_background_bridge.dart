import 'package:flutter/services.dart';

import 'host_models.dart';

typedef WakeAndDrainHandler = Future<void> Function(String reason);
typedef CancelWakeAndDrainHandler = Future<void> Function(String reason);

class PlatformBackgroundBridge {
  PlatformBackgroundBridge({MethodChannel? channel})
      : _channel = channel ??
            const MethodChannel('ores_common_mobile_host/background') {
    _channel.setMethodCallHandler(_handleNativeCall);
  }

  final MethodChannel _channel;

  WakeAndDrainHandler? _wakeAndDrainHandler;
  CancelWakeAndDrainHandler? _cancelWakeAndDrainHandler;

  void setWakeAndDrainHandler(WakeAndDrainHandler handler) {
    _wakeAndDrainHandler = handler;
  }

  void setCancelWakeAndDrainHandler(CancelWakeAndDrainHandler handler) {
    _cancelWakeAndDrainHandler = handler;
  }

  Future<MobileHostCapabilities> capabilities() async {
    final Map<Object?, Object?>? result =
        await _channel.invokeMapMethod<Object?, Object?>('capabilities');

    return MobileHostCapabilities.fromMap(
      result ?? const <Object?, Object?>{},
    );
  }

  Future<void> startPersistentHosting(MobileHostConfig config) async {
    config.validate();

    await _channel.invokeMethod<void>('startPersistentHosting', <String, Object?>{
      'product_id': config.productId,
      'device_id': config.deviceId,
      'origin_port': config.originPort,
      'relay_url': config.relayUrl.toString(),
    });
  }

  Future<void> stopPersistentHosting() async {
    await _channel.invokeMethod<void>('stopPersistentHosting');
  }

  Future<void> scheduleRepairWake({required Duration minimumDelay}) async {
    await _channel.invokeMethod<void>('scheduleRepairWake', <String, Object?>{
      'minimum_delay_seconds': minimumDelay.inSeconds,
    });
  }

  Future<void> configureKeepalivePrompts(KeepalivePromptPolicy policy) async {
    policy.validate();

    await _channel.invokeMethod<void>('configureKeepalivePrompts', <String, Object?>{
      'enabled': policy.enabled,
      'prompts_per_day': policy.promptsPerDay,
    });
  }

  Future<Object?> _handleNativeCall(MethodCall call) async {
    final arguments = call.arguments as Map<Object?, Object?>?;
    final reason = arguments?['reason'] as String? ?? 'platform_wake';

    switch (call.method) {
      case 'wakeAndDrain':
        final handler = _wakeAndDrainHandler;

        if (handler == null) {
          throw PlatformException(
            code: 'wake_handler_missing',
            message: 'No Dart wake-and-drain handler has been registered',
          );
        }

        await handler(reason);
        return true;

      case 'cancelWakeAndDrain':
        final handler = _cancelWakeAndDrainHandler;

        if (handler != null) {
          await handler(reason);
        }

        return true;

      default:
        throw MissingPluginException(
          'Unsupported native mobile-host callback: ${call.method}',
        );
    }
  }
}
