import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'host_models.dart';
import 'mobile_host_coordinator.dart';

typedef AccessTokenProvider = Future<String> Function();

class CloudflareRelayClient implements MobileRelayClient {
  CloudflareRelayClient({
    required AccessTokenProvider accessTokenProvider,
    this.maxRequestBodyBytes = 4 * 1024 * 1024,
    this.maxResponseBodyBytes = 4 * 1024 * 1024,
    this.maxQueuedRequests = 64,
    this.requestTimeout = const Duration(seconds: 20),
  }) : _accessTokenProvider = accessTokenProvider {
    if (maxRequestBodyBytes <= 0 || maxResponseBodyBytes <= 0) {
      throw ArgumentError('relay body limits must be positive');
    }

    if (maxQueuedRequests <= 0) {
      throw ArgumentError.value(
        maxQueuedRequests,
        'maxQueuedRequests',
        'must be positive',
      );
    }

    if (requestTimeout <= Duration.zero) {
      throw ArgumentError.value(
        requestTimeout,
        'requestTimeout',
        'must be positive',
      );
    }
  }

  static const Set<String> _allowedMethods = <String>{
    'GET',
    'HEAD',
    'POST',
    'PUT',
    'PATCH',
    'DELETE',
    'OPTIONS',
  };

  static const List<int> _reconnectBackoffSeconds = <int>[1, 2, 4, 8, 16, 30];

  final AccessTokenProvider _accessTokenProvider;
  final int maxRequestBodyBytes;
  final int maxResponseBodyBytes;
  final int maxQueuedRequests;
  final Duration requestTimeout;

  WebSocket? _socket;
  HttpClient? _drainClient;
  bool _drainCancelled = false;
  bool _persistentDesired = false;
  MobileHostConfig? _persistentConfig;
  Uri? _persistentLocalOrigin;
  Timer? _reconnectTimer;
  int _persistentGeneration = 0;
  int _reconnectAttempt = 0;

  @override
  Future<void> openPersistentSession({
    required MobileHostConfig config,
    required Uri localOrigin,
  }) async {
    config.validate();
    _validateLocalOrigin(localOrigin);

    _persistentGeneration += 1;
    final generation = _persistentGeneration;
    _persistentDesired = true;
    _persistentConfig = config;
    _persistentLocalOrigin = localOrigin;
    _reconnectAttempt = 0;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;

    await _closeSocketOnly();
    await _connectPersistentSession(
      config: config,
      localOrigin: localOrigin,
      generation: generation,
    );
  }

  @override
  Future<void> closePersistentSession() async {
    _persistentDesired = false;
    _persistentGeneration += 1;
    _persistentConfig = null;
    _persistentLocalOrigin = null;
    _reconnectAttempt = 0;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;

    await _closeSocketOnly();
  }

  @override
  Future<void> drainQueuedRequests({
    required MobileHostConfig config,
    required Uri localOrigin,
    required Duration budget,
  }) async {
    config.validate();
    _validateLocalOrigin(localOrigin);

    if (budget <= Duration.zero) {
      throw ArgumentError.value(budget, 'budget', 'must be positive');
    }

    _drainCancelled = false;

    final token = await _accessToken();
    final client = _newHttpClient();
    _drainClient = client;
    final deadline = DateTime.now().add(budget);

    try {
      final request = await client
          .postUrl(_httpEndpoint(config.relayUrl, 'drain'))
          .timeout(requestTimeout);
      request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
      request.headers.contentType = ContentType.json;
      request.write(
        jsonEncode(<String, Object?>{
          'product_id': config.productId,
          'device_id': config.deviceId,
          'deadline_epoch_ms': deadline.millisecondsSinceEpoch,
        }),
      );

      final response = await request.close().timeout(requestTimeout);
      final body = await _readBoundedResponse(
        response,
        maxRequestBodyBytes * 2,
      ).timeout(requestTimeout);

      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw HttpException(
          'relay drain failed with ${response.statusCode}',
          uri: config.relayUrl,
        );
      }

      final decoded = jsonDecode(utf8.decode(body));

      if (decoded is! Map<String, dynamic>) {
        throw const FormatException('relay drain response must be an object');
      }

      final requests = decoded['requests'];

      if (requests is! List<dynamic>) {
        throw const FormatException('relay drain response must contain requests');
      }

      if (requests.length > maxQueuedRequests) {
        throw FormatException(
          'relay drain returned ${requests.length} requests; maximum is $maxQueuedRequests',
        );
      }

      for (final item in requests) {
        if (_drainCancelled || DateTime.now().isAfter(deadline)) {
          break;
        }

        if (item is! Map<String, dynamic>) {
          throw const FormatException('queued relay request must be an object');
        }

        final relayResponse = await _forwardToLocalOrigin(
          localOrigin: localOrigin,
          message: item,
        );

        await _postQueuedResponse(
          client: client,
          config: config,
          token: token,
          relayResponse: relayResponse,
        );
      }
    } finally {
      if (identical(_drainClient, client)) {
        _drainClient = null;
      }

      client.close(force: true);
    }
  }

  @override
  Future<void> cancelDrain() async {
    _drainCancelled = true;

    final client = _drainClient;
    _drainClient = null;
    client?.close(force: true);
  }

  Future<void> _connectPersistentSession({
    required MobileHostConfig config,
    required Uri localOrigin,
    required int generation,
  }) async {
    final token = await _accessToken();
    final socket = await WebSocket.connect(
      _webSocketEndpoint(config.relayUrl, 'session').toString(),
      headers: <String, dynamic>{
        HttpHeaders.authorizationHeader: 'Bearer $token',
        'x-ores-product-id': config.productId,
        'x-ores-device-id': config.deviceId,
      },
    ).timeout(requestTimeout);

    if (!_persistentDesired || generation != _persistentGeneration) {
      await socket.close(WebSocketStatus.goingAway, 'session superseded');
      return;
    }

    _socket = socket;
    _reconnectAttempt = 0;

    socket.add(
      jsonEncode(<String, Object?>{
        'type': 'hello',
        'product_id': config.productId,
        'device_id': config.deviceId,
        'origin_port': config.originPort,
      }),
    );

    socket.listen(
      (dynamic data) {
        unawaited(
          _handlePersistentMessage(
            socket: socket,
            localOrigin: localOrigin,
            data: data,
          ),
        );
      },
      onDone: () {
        _handleSocketClosed(socket, generation);
      },
      onError: (Object _) {
        _handleSocketClosed(socket, generation);
      },
      cancelOnError: false,
    );
  }

  void _handleSocketClosed(WebSocket socket, int generation) {
    if (identical(_socket, socket)) {
      _socket = null;
    }

    if (!_persistentDesired || generation != _persistentGeneration) {
      return;
    }

    _scheduleReconnect(generation);
  }

  void _scheduleReconnect(int generation) {
    if (_reconnectTimer?.isActive ?? false) {
      return;
    }

    final index = _reconnectAttempt < _reconnectBackoffSeconds.length
        ? _reconnectAttempt
        : _reconnectBackoffSeconds.length - 1;
    final delay = Duration(seconds: _reconnectBackoffSeconds[index]);
    _reconnectAttempt += 1;

    _reconnectTimer = Timer(delay, () {
      _reconnectTimer = null;
      unawaited(_reconnectPersistent(generation));
    });
  }

  Future<void> _reconnectPersistent(int generation) async {
    if (!_persistentDesired || generation != _persistentGeneration) {
      return;
    }

    final config = _persistentConfig;
    final localOrigin = _persistentLocalOrigin;

    if (config == null || localOrigin == null) {
      return;
    }

    try {
      await _connectPersistentSession(
        config: config,
        localOrigin: localOrigin,
        generation: generation,
      );
    } catch (_) {
      if (_persistentDesired && generation == _persistentGeneration) {
        _scheduleReconnect(generation);
      }
    }
  }

  Future<void> _closeSocketOnly() async {
    final socket = _socket;
    _socket = null;

    if (socket != null) {
      await socket.close(WebSocketStatus.normalClosure, 'hosting stopped');
    }
  }

  Future<void> _handlePersistentMessage({
    required WebSocket socket,
    required Uri localOrigin,
    required dynamic data,
  }) async {
    if (data is! String) {
      return;
    }

    if (utf8.encode(data).length > maxRequestBodyBytes * 2) {
      _sendProtocolError(socket, 'message_too_large');
      return;
    }

    Map<String, dynamic> decoded;

    try {
      final value = jsonDecode(data);

      if (value is! Map<String, dynamic>) {
        _sendProtocolError(socket, 'invalid_message');
        return;
      }

      decoded = value;
    } on FormatException {
      _sendProtocolError(socket, 'invalid_json');
      return;
    }

    if (decoded['type'] != 'request') {
      return;
    }

    try {
      final response = await _forwardToLocalOrigin(
        localOrigin: localOrigin,
        message: decoded,
      );

      socket.add(jsonEncode(response));
    } catch (_) {
      socket.add(
        jsonEncode(<String, Object?>{
          'type': 'response',
          'request_id': decoded['request_id'],
          'status': HttpStatus.badGateway,
          'headers': <String, String>{
            HttpHeaders.contentTypeHeader: 'text/plain; charset=utf-8',
          },
          'body_base64': base64Encode(utf8.encode('local origin failed')),
        }),
      );
    }
  }

  Future<Map<String, Object?>> _forwardToLocalOrigin({
    required Uri localOrigin,
    required Map<String, dynamic> message,
  }) async {
    _validateLocalOrigin(localOrigin);

    final requestId = message['request_id'] as String?;
    final method = (message['method'] as String? ?? 'GET').toUpperCase();
    final path = message['path'] as String? ?? '/';
    final encodedBody = message['body_base64'] as String? ?? '';
    final deadlineEpochMs = message['deadline_epoch_ms'] as num?;
    final requestBody = base64Decode(encodedBody);

    if (requestId == null || requestId.isEmpty) {
      throw const FormatException('relay request_id is required');
    }

    if (!_allowedMethods.contains(method)) {
      throw FormatException('relay HTTP method is not allowed: $method');
    }

    final pathUri = Uri.tryParse(path);

    if (pathUri == null ||
        !path.startsWith('/') ||
        pathUri.hasScheme ||
        pathUri.hasAuthority ||
        pathUri.hasFragment) {
      throw const FormatException('relay path must be origin-relative');
    }

    if (deadlineEpochMs != null &&
        DateTime.now().millisecondsSinceEpoch > deadlineEpochMs.toInt()) {
      throw TimeoutException('relay request deadline has expired');
    }

    if (requestBody.length > maxRequestBodyBytes) {
      throw StateError('relay request body exceeds configured limit');
    }

    final uri = localOrigin.replace(
      path: pathUri.path,
      query: pathUri.hasQuery ? pathUri.query : null,
      fragment: null,
    );
    final client = _newHttpClient();

    try {
      final request = await client.openUrl(method, uri).timeout(requestTimeout);
      final rawHeaders = message['headers'];

      if (rawHeaders != null && rawHeaders is! Map<String, dynamic>) {
        throw const FormatException('relay headers must be an object');
      }

      if (rawHeaders is Map<String, dynamic>) {
        for (final entry in rawHeaders.entries) {
          if (_isForbiddenForwardHeader(entry.key)) {
            continue;
          }

          final value = entry.value;

          if (value is! String) {
            throw FormatException(
              'relay header ${entry.key} must contain a string value',
            );
          }

          request.headers.set(entry.key, value);
        }
      }

      if (requestBody.isNotEmpty) {
        request.add(requestBody);
      }

      final response = await request.close().timeout(requestTimeout);
      final responseBody = await _readBoundedResponse(
        response,
        maxResponseBodyBytes,
      ).timeout(requestTimeout);
      final headers = <String, String>{};

      response.headers.forEach((String name, List<String> values) {
        if (_isForbiddenForwardHeader(name)) {
          return;
        }

        headers[name] = values.join(', ');
      });

      return <String, Object?>{
        'type': 'response',
        'request_id': requestId,
        'status': response.statusCode,
        'headers': headers,
        'body_base64': base64Encode(responseBody),
      };
    } finally {
      client.close(force: true);
    }
  }

  Future<void> _postQueuedResponse({
    required HttpClient client,
    required MobileHostConfig config,
    required String token,
    required Map<String, Object?> relayResponse,
  }) async {
    final request = await client
        .postUrl(_httpEndpoint(config.relayUrl, 'response'))
        .timeout(requestTimeout);
    request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
    request.headers.contentType = ContentType.json;
    request.write(jsonEncode(relayResponse));

    final response = await request.close().timeout(requestTimeout);
    await response.drain<void>().timeout(requestTimeout);

    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw HttpException(
        'relay response upload failed with ${response.statusCode}',
        uri: config.relayUrl,
      );
    }
  }

  Future<List<int>> _readBoundedResponse(
    HttpClientResponse response,
    int limit,
  ) async {
    final bytes = <int>[];

    await for (final chunk in response) {
      if (bytes.length + chunk.length > limit) {
        throw StateError('relay response body exceeds configured limit');
      }

      bytes.addAll(chunk);
    }

    return bytes;
  }

  Future<String> _accessToken() async {
    final token = await _accessTokenProvider();

    if (token.trim().isEmpty) {
      throw StateError('relay access token provider returned an empty token');
    }

    return token;
  }

  HttpClient _newHttpClient() {
    return HttpClient()..connectionTimeout = requestTimeout;
  }

  void _validateLocalOrigin(Uri localOrigin) {
    final address = InternetAddress.tryParse(localOrigin.host);
    final isLoopbackHost = localOrigin.host == 'localhost' ||
        (address != null && address.isLoopback);

    if (localOrigin.scheme != 'http' ||
        !isLoopbackHost ||
        !localOrigin.hasPort ||
        localOrigin.port <= 0 ||
        localOrigin.userInfo.isNotEmpty ||
        localOrigin.hasQuery ||
        localOrigin.hasFragment) {
      throw ArgumentError.value(
        localOrigin,
        'localOrigin',
        'must be an HTTP loopback origin with an explicit non-zero port',
      );
    }
  }

  void _sendProtocolError(WebSocket socket, String code) {
    socket.add(
      jsonEncode(<String, Object?>{
        'type': 'protocol_error',
        'code': code,
      }),
    );
  }

  bool _isForbiddenForwardHeader(String name) {
    final normalized = name.toLowerCase();

    if (normalized.startsWith('x-ores-')) {
      return true;
    }

    switch (normalized) {
      case 'connection':
      case 'content-length':
      case 'host':
      case 'keep-alive':
      case 'proxy-authenticate':
      case 'proxy-authorization':
      case 'te':
      case 'trailer':
      case 'transfer-encoding':
      case 'upgrade':
        return true;
      default:
        return false;
    }
  }

  Uri _httpEndpoint(Uri base, String child) {
    final path = base.path.endsWith('/')
        ? '${base.path}$child'
        : '${base.path}/$child';

    return base.replace(path: path);
  }

  Uri _webSocketEndpoint(Uri base, String child) {
    final endpoint = _httpEndpoint(base, child);

    return endpoint.replace(scheme: 'wss');
  }
}
