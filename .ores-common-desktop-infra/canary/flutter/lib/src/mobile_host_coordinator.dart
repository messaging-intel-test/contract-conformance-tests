import 'dart:async';
import 'dart:io';

import 'host_models.dart';
import 'platform_background_bridge.dart';

typedef MobileHostConfigLoader = Future<MobileHostConfig> Function();

abstract interface class LocalOriginHandler {
  Future<void> handle(HttpRequest request);
}

abstract interface class MobileRelayClient {
  Future<void> openPersistentSession({
    required MobileHostConfig config,
    required Uri localOrigin,
  });

  Future<void> closePersistentSession();

  Future<void> drainQueuedRequests({
    required MobileHostConfig config,
    required Uri localOrigin,
    required Duration budget,
  });

  Future<void> cancelDrain();
}

class MobileHostCoordinator {
  MobileHostCoordinator({
    required PlatformBackgroundBridge backgroundBridge,
    required MobileRelayClient relayClient,
    required LocalOriginHandler originHandler,
  })  : _backgroundBridge = backgroundBridge,
        _relayClient = relayClient,
        _originHandler = originHandler;

  final PlatformBackgroundBridge _backgroundBridge;
  final MobileRelayClient _relayClient;
  final LocalOriginHandler _originHandler;

  HttpServer? _server;
  MobileHostConfig? _config;

  Future<MobileHostCapabilities> capabilities() async {
    return _backgroundBridge.capabilities();
  }

  void installNativeWakeHandler({
    required MobileHostConfigLoader loadConfig,
    Duration budget = const Duration(seconds: 20),
  }) {
    _backgroundBridge.setWakeAndDrainHandler((String _reason) async {
      final config = await loadConfig();
      await runWakeAndDrainOnce(config: config, budget: budget);
    });

    _backgroundBridge.setCancelWakeAndDrainHandler((String _reason) async {
      await _relayClient.cancelDrain();
      await _closeOrigin();
    });
  }

  Future<void> startPersistent(MobileHostConfig config) async {
    config.validate();

    final capabilities = await _backgroundBridge.capabilities();

    if (!capabilities.supportsPersistentOrigin) {
      throw StateError(
        '${capabilities.platform} does not support persistent background origin hosting',
      );
    }

    if (config.mode != MobileHostingMode.persistentOrigin) {
      throw StateError('persistent start requires persistentOrigin mode');
    }

    await _bindOrigin(config);
    await _backgroundBridge.startPersistentHosting(config);

    try {
      await _relayClient.openPersistentSession(
        config: config,
        localOrigin: _localOrigin(config),
      );
    } catch (_) {
      await stop();
      rethrow;
    }
  }

  Future<void> runWakeAndDrainOnce({
    required MobileHostConfig config,
    Duration budget = const Duration(seconds: 20),
  }) async {
    config.validate();

    final capabilities = await _backgroundBridge.capabilities();

    if (!capabilities.supportsWakeAndDrain) {
      throw StateError(
        '${capabilities.platform} does not support wake-and-drain hosting',
      );
    }

    if (config.mode != MobileHostingMode.wakeAndDrain) {
      throw StateError('wake-and-drain requires wakeAndDrain mode');
    }

    await _bindOrigin(config);

    try {
      await _relayClient.drainQueuedRequests(
        config: config,
        localOrigin: _localOrigin(config),
        budget: budget,
      );
    } finally {
      await _closeOrigin();
    }
  }

  Future<void> scheduleRepairWake(Duration minimumDelay) async {
    await _backgroundBridge.scheduleRepairWake(minimumDelay: minimumDelay);
  }

  Future<void> stop() async {
    await _relayClient.cancelDrain();
    await _relayClient.closePersistentSession();
    await _backgroundBridge.stopPersistentHosting();
    await _closeOrigin();
  }

  Future<void> _bindOrigin(MobileHostConfig config) async {
    if (_server != null) {
      if (_config?.originPort == config.originPort) {
        return;
      }

      throw StateError('mobile origin is already bound to another port');
    }

    final server = await HttpServer.bind(
      InternetAddress.loopbackIPv4,
      config.originPort,
      shared: false,
    );

    _config = config;
    _server = server;

    unawaited(_serve(server));
  }

  Future<void> _serve(HttpServer server) async {
    await for (final request in server) {
      try {
        await _originHandler.handle(request);
      } catch (error) {
        request.response.statusCode = HttpStatus.internalServerError;
        request.response.write('local origin handler failed: $error');
        await request.response.close();
      }
    }
  }

  Future<void> _closeOrigin() async {
    final server = _server;

    _server = null;
    _config = null;

    if (server != null) {
      await server.close(force: true);
    }
  }

  Uri _localOrigin(MobileHostConfig config) {
    return Uri.parse('http://127.0.0.1:${config.originPort}');
  }
}
