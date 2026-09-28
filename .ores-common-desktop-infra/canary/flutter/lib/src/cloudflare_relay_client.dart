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
  }) : _accessTokenProvider = accessTokenProvider;

  static const Set<String> _allowedMethods = <String>{
    'GET','HEAD','POST','PUT','PATCH','DELETE','OPTIONS',
  };

  final AccessTokenProvider _accessTokenProvider;
  final int maxRequestBodyBytes;
  final int maxResponseBodyBytes;
  WebSocket? _socket;
  HttpClient? _drainClient;
  bool _drainCancelled = false;

  @override
  Future<void> openPersistentSession({required MobileHostConfig config, required Uri localOrigin}) async {
    await closePersistentSession();
    final token = await _accessTokenProvider();
    final socket = await WebSocket.connect(
      _webSocketEndpoint(config.relayUrl, 'session').toString(),
      headers: <String, dynamic>{
        HttpHeaders.authorizationHeader: 'Bearer $token',
        'x-ores-product-id': config.productId,
        'x-ores-device-id': config.deviceId,
      },
    );
    _socket = socket;
    socket.add(jsonEncode(<String,Object?>{'type':'hello','product_id':config.productId,'device_id':config.deviceId,'origin_port':config.originPort}));
    socket.listen((dynamic data) { unawaited(_handlePersistentMessage(socket: socket, localOrigin: localOrigin, data: data)); }, onDone: () { if (identical(_socket, socket)) { _socket = null; } }, onError: (Object _) { if (identical(_socket, socket)) { _socket = null; } }, cancelOnError: false);
  }

  @override
  Future<void> closePersistentSession() async { final socket = _socket; _socket = null; if (socket != null) { await socket.close(WebSocketStatus.normalClosure, 'hosting stopped'); } }

  @override
  Future<void> drainQueuedRequests({required MobileHostConfig config, required Uri localOrigin, required Duration budget}) async {
    _drainCancelled = false;
    final token = await _accessTokenProvider();
    final client = HttpClient();
    _drainClient = client;
    final deadline = DateTime.now().add(budget);
    try {
      final request = await client.postUrl(_httpEndpoint(config.relayUrl, 'drain'));
      request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
      request.headers.contentType = ContentType.json;
      request.write(jsonEncode(<String,Object?>{'product_id':config.productId,'device_id':config.deviceId,'deadline_epoch_ms':deadline.millisecondsSinceEpoch}));
      final response = await request.close();
      final body = await _readBoundedResponse(response, maxRequestBodyBytes * 2);
      if (response.statusCode < 200 || response.statusCode >= 300) { throw HttpException('relay drain failed with ${response.statusCode}', uri: config.relayUrl); }
      final decoded = jsonDecode(utf8.decode(body));
      if (decoded is! Map<String,dynamic>) { throw const FormatException('relay drain response must be an object'); }
      final requests = decoded['requests'];
      if (requests is! List<dynamic>) { throw const FormatException('relay drain response must contain requests'); }
      for (final item in requests) {
        if (_drainCancelled || DateTime.now().isAfter(deadline)) { break; }
        if (item is! Map<String,dynamic>) { continue; }
        final relayResponse = await _forwardToLocalOrigin(localOrigin: localOrigin, message: item);
        await _postQueuedResponse(client: client, config: config, token: token, relayResponse: relayResponse);
      }
    } finally { if (identical(_drainClient, client)) { _drainClient = null; } client.close(force: true); }
  }

  @override
  Future<void> cancelDrain() async { _drainCancelled = true; final client = _drainClient; _drainClient = null; client?.close(force: true); }

  Future<void> _handlePersistentMessage({required WebSocket socket, required Uri localOrigin, required dynamic data}) async {
    if (data is! String) { return; }
    if (utf8.encode(data).length > maxRequestBodyBytes * 2) { socket.add(jsonEncode(<String,Object?>{'type':'protocol_error','code':'message_too_large'})); return; }
    final decoded = jsonDecode(data);
    if (decoded is! Map<String,dynamic> || decoded['type'] != 'request') { return; }
    try { final response = await _forwardToLocalOrigin(localOrigin: localOrigin, message: decoded); socket.add(jsonEncode(response)); }
    catch (error) { socket.add(jsonEncode(<String,Object?>{'type':'response','request_id':decoded['request_id'],'status':HttpStatus.badGateway,'headers':<String,String>{HttpHeaders.contentTypeHeader:'text/plain; charset=utf-8'},'body_base64':base64Encode(utf8.encode('local origin failed: $error'))})); }
  }

  Future<Map<String,Object?>> _forwardToLocalOrigin({required Uri localOrigin, required Map<String,dynamic> message}) async {
    final requestId = message['request_id'] as String?;
    final method = (message['method'] as String? ?? 'GET').toUpperCase();
    final path = message['path'] as String? ?? '/';
    final encodedBody = message['body_base64'] as String? ?? '';
    final deadlineEpochMs = message['deadline_epoch_ms'] as num?;
    final requestBody = base64Decode(encodedBody);
    if (requestId == null || requestId.isEmpty) { throw const FormatException('relay request_id is required'); }
    if (!_allowedMethods.contains(method)) { throw FormatException('relay HTTP method is not allowed: $method'); }
    final pathUri = Uri.tryParse(path);
    if (pathUri == null || !path.startsWith('/') || pathUri.hasScheme || pathUri.hasAuthority) { throw const FormatException('relay path must be origin-relative'); }
    if (deadlineEpochMs != null && DateTime.now().millisecondsSinceEpoch > deadlineEpochMs.toInt()) { throw TimeoutException('relay request deadline has expired'); }
    if (requestBody.length > maxRequestBodyBytes) { throw StateError('relay request body exceeds configured limit'); }
    final uri = localOrigin.replace(path: pathUri.path, query: pathUri.hasQuery ? pathUri.query : null, fragment: null);
    final client = HttpClient();
    try {
      final request = await client.openUrl(method, uri);
      final rawHeaders = message['headers'];
      if (rawHeaders is Map<String,dynamic>) { for (final entry in rawHeaders.entries) { if (_isHopByHopHeader(entry.key)) { continue; } final value = entry.value; if (value is String) { request.headers.set(entry.key, value); } } }
      if (requestBody.isNotEmpty) { request.add(requestBody); }
      final response = await request.close();
      final responseBody = await _readBoundedResponse(response, maxResponseBodyBytes);
      final headers = <String,String>{};
      response.headers.forEach((String name, List<String> values) { if (_isHopByHopHeader(name)) { return; } headers[name] = values.join(', '); });
      return <String,Object?>{'type':'response','request_id':requestId,'status':response.statusCode,'headers':headers,'body_base64':base64Encode(responseBody)};
    } finally { client.close(force: true); }
  }

  Future<void> _postQueuedResponse({required HttpClient client, required MobileHostConfig config, required String token, required Map<String,Object?> relayResponse}) async {
    final request = await client.postUrl(_httpEndpoint(config.relayUrl, 'response'));
    request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token'); request.headers.contentType = ContentType.json; request.write(jsonEncode(relayResponse));
    final response = await request.close(); await response.drain<void>();
    if (response.statusCode < 200 || response.statusCode >= 300) { throw HttpException('relay response upload failed with ${response.statusCode}', uri: config.relayUrl); }
  }

  Future<List<int>> _readBoundedResponse(HttpClientResponse response, int limit) async { final bytes=<int>[]; await for (final chunk in response) { if (bytes.length + chunk.length > limit) { throw StateError('relay response body exceeds configured limit'); } bytes.addAll(chunk); } return bytes; }
  bool _isHopByHopHeader(String name) { switch(name.toLowerCase()) { case 'connection': case 'content-length': case 'host': case 'keep-alive': case 'proxy-authenticate': case 'proxy-authorization': case 'te': case 'trailer': case 'transfer-encoding': case 'upgrade': return true; default: return false; } }
  Uri _httpEndpoint(Uri base, String child) { final path = base.path.endsWith('/') ? '${base.path}$child' : '${base.path}/$child'; return base.replace(path:path); }
  Uri _webSocketEndpoint(Uri base, String child) { final endpoint = _httpEndpoint(base, child); final scheme = endpoint.scheme == 'https' ? 'wss' : 'ws'; return endpoint.replace(scheme:scheme); }
}
