import 'dart:convert';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:http/http.dart' as http;

class BunnyApi {
  BunnyApi({
    required this.baseUrl,
    FlutterSecureStorage? storage,
    this.profileId,
  }) : _storage = storage ?? const FlutterSecureStorage();

  String baseUrl;
  final FlutterSecureStorage _storage;
  final String? profileId;

  static const _legacyTokenKey = 'bunny_token';

  Future<void> saveToken(String token) async {
    if (profileId != null) {
      await _storage.write(key: 'bunny_token_$profileId', value: token);
    } else {
      await _storage.write(key: _legacyTokenKey, value: token);
    }
  }

  Future<String?> getToken() async {
    if (profileId != null) {
      return _storage.read(key: 'bunny_token_$profileId');
    }
    return _storage.read(key: _legacyTokenKey);
  }

  Future<void> clearToken() async {
    if (profileId != null) {
      await _storage.delete(key: 'bunny_token_$profileId');
    } else {
      await _storage.delete(key: _legacyTokenKey);
    }
  }

  Map<String, String> get _headers => {'Content-Type': 'application/json'};

  Future<Map<String, String>> _authHeaders() async {
    final token = await getToken();
    return {
      ..._headers,
      if (token != null) 'Authorization': 'Bearer $token',
    };
  }

  /// Public agent discovery — no auth (for tunnel verification).
  Future<AgentInfo> agentInfo() async {
    final res = await http
        .get(Uri.parse('$baseUrl/api/v1/agent/info'))
        .timeout(const Duration(seconds: 8));
    if (res.statusCode != 200) {
      throw Exception('Agent not reachable (${res.statusCode})');
    }
    return AgentInfo.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
  }

  Future<Map<String, dynamic>> login(String email, String password) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/auth/login'),
      headers: _headers,
      body: jsonEncode({
        'email': email,
        'password': password,
        'device_id': 'mobile',
      }),
    );
    if (res.statusCode != 200) {
      throw Exception(_errorMessage(res));
    }
    final cookies = res.headers['set-cookie'];
    if (cookies != null && cookies.contains('bunny_session=')) {
      final token = cookies.split('bunny_session=')[1].split(';').first;
      await saveToken(token);
    }
    final body = jsonDecode(res.body) as Map<String, dynamic>;
    final tokenHeader = res.headers['x-bunny-token'];
    if (tokenHeader != null) {
      await saveToken(tokenHeader);
    }
    return body;
  }

  Future<Map<String, dynamic>> me() async {
    final res = await http.get(
      Uri.parse('$baseUrl/api/v1/auth/me'),
      headers: await _authHeaders(),
    );
    if (res.statusCode != 200) throw Exception('Unauthorized');
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  Future<Map<String, dynamic>> createSession({String? projectPath}) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/sessions'),
      headers: await _authHeaders(),
      body: jsonEncode({
        if (projectPath != null) 'project_path': projectPath,
      }),
    );
    if (res.statusCode != 200) throw Exception('Failed to create session');
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  Future<List<dynamic>> listSessions() async {
    final res = await http.get(
      Uri.parse('$baseUrl/api/v1/sessions'),
      headers: await _authHeaders(),
    );
    if (res.statusCode != 200) throw Exception('Failed to list sessions');
    return jsonDecode(res.body) as List<dynamic>;
  }

  Future<Map<String, dynamic>> createTerminal(
    String sessionId,
    String name, {
    String? command,
  }) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/terminals'),
      headers: await _authHeaders(),
      body: jsonEncode({
        'session_id': sessionId,
        'name': name,
        if (command != null) 'command': command,
        'cols': 80,
        'rows': 24,
      }),
    );
    if (res.statusCode != 200) throw Exception('Failed to create terminal');
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  String sessionRealtimeWsUrl(String sessionId, {int lastEventId = 0}) {
    final uri = Uri.parse(baseUrl);
    final wsScheme = uri.scheme == 'https' ? 'wss' : 'ws';
    final host = uri.host.isEmpty ? '127.0.0.1' : uri.host;
    final port = uri.hasPort ? uri.port : (wsScheme == 'wss' ? 443 : 80);
    final q = lastEventId > 0 ? '?lastEventId=$lastEventId' : '';
    return '$wsScheme://$host:$port/api/v1/sessions/$sessionId/realtime$q';
  }

  Future<WebRtcConfig> webrtcConfig() async {
    final res = await http.get(
      Uri.parse('$baseUrl/api/v1/webrtc/config'),
      headers: await _authHeaders(),
    );
    if (res.statusCode != 200) throw Exception('WebRTC config failed');
    return WebRtcConfig.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
  }

  Future<SdpAnswer> webrtcOffer(String sessionId, SdpOffer offer) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/sessions/$sessionId/webrtc/offer'),
      headers: await _authHeaders(),
      body: jsonEncode({
        'type': offer.type,
        'sdp': offer.sdp,
      }),
    );
    if (res.statusCode != 200) throw Exception('WebRTC offer failed');
    final body = jsonDecode(res.body) as Map<String, dynamic>;
    return SdpAnswer(
      type: body['type'] as String? ?? 'answer',
      sdp: body['sdp'] as String? ?? '',
    );
  }

  Future<void> webrtcCandidate(
    String sessionId,
    Map<String, dynamic> candidate,
  ) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/sessions/$sessionId/webrtc/candidate'),
      headers: await _authHeaders(),
      body: jsonEncode({'candidate': candidate}),
    );
    if (res.statusCode != 204 && res.statusCode != 200) {
      throw Exception('WebRTC candidate failed');
    }
  }

  Future<Map<String, dynamic>> createBrowser(
    String sessionId, {
    String? targetUrl,
  }) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/browser-sessions'),
      headers: await _authHeaders(),
      body: jsonEncode({
        'session_id': sessionId,
        if (targetUrl != null) 'target_url': targetUrl,
      }),
    );
    if (res.statusCode != 200) throw Exception('Failed to create browser');
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  Future<Map<String, dynamic>> getBrowser(String browserId) async {
    final res = await http.get(
      Uri.parse('$baseUrl/api/v1/browser-sessions/$browserId'),
      headers: await _authHeaders(),
    );
    if (res.statusCode != 200) throw Exception('Failed to get browser');
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  Future<SdpAnswer> browserWebrtcOffer(String browserId, SdpOffer offer) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/browser-sessions/$browserId/webrtc/offer'),
      headers: await _authHeaders(),
      body: jsonEncode({
        'type': offer.type,
        'sdp': offer.sdp,
      }),
    );
    if (res.statusCode != 200) throw Exception('Browser WebRTC offer failed');
    final body = jsonDecode(res.body) as Map<String, dynamic>;
    return SdpAnswer(
      type: body['type'] as String? ?? 'answer',
      sdp: body['sdp'] as String? ?? '',
    );
  }

  Future<void> browserWebrtcCandidate(
    String browserId,
    Map<String, dynamic> candidate,
  ) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/browser-sessions/$browserId/webrtc/candidate'),
      headers: await _authHeaders(),
      body: jsonEncode({'candidate': candidate}),
    );
    if (res.statusCode != 204 && res.statusCode != 200) {
      throw Exception('Browser WebRTC candidate failed');
    }
  }

  Future<void> browserWebrtcStop(String browserId) async {
    await http.post(
      Uri.parse('$baseUrl/api/v1/browser-sessions/$browserId/webrtc/stop'),
      headers: await _authHeaders(),
    );
  }

  Future<bool> registerPush({
    required String deviceId,
    required String platform,
    required String token,
  }) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/push/register'),
      headers: await _authHeaders(),
      body: jsonEncode({
        'device_id': deviceId,
        'platform': platform,
        'provider': 'fcm',
        'token': token,
      }),
    );
    if (res.statusCode != 200) throw Exception('Push register failed');
    final body = jsonDecode(res.body) as Map<String, dynamic>;
    return body['fcm_configured'] as bool? ?? false;
  }

  String terminalWsUrl(String terminalId, {int fromOffset = 0}) {
    final uri = Uri.parse(baseUrl);
    final wsScheme = uri.scheme == 'https' ? 'wss' : 'ws';
    final host = uri.host.isEmpty ? '127.0.0.1' : uri.host;
    final port = uri.hasPort ? uri.port : (wsScheme == 'wss' ? 443 : 80);
    return '$wsScheme://$host:$port/api/v1/terminals/$terminalId/ws?from_offset=$fromOffset';
  }

  String _errorMessage(http.Response res) {
    try {
      final body = jsonDecode(res.body) as Map<String, dynamic>;
      return body['error']?['message'] as String? ?? res.statusCode.toString();
    } catch (_) {
      return res.statusCode.toString();
    }
  }
}

class WebRtcConfig {
  WebRtcConfig({
    required this.enabled,
    required this.iceServers,
  });

  final bool enabled;
  final List<Map<String, dynamic>> iceServers;

  factory WebRtcConfig.fromJson(Map<String, dynamic> json) {
    final servers = (json['ice_servers'] as List<dynamic>? ?? [])
        .map((e) => Map<String, dynamic>.from(e as Map))
        .toList();
    return WebRtcConfig(
      enabled: json['enabled'] as bool? ?? false,
      iceServers: servers,
    );
  }
}

class SdpOffer {
  SdpOffer({required this.type, required this.sdp});
  final String type;
  final String sdp;
}

class SdpAnswer {
  SdpAnswer({required this.type, required this.sdp});
  final String type;
  final String sdp;
}

class AgentInfo {
  AgentInfo({
    required this.name,
    required this.apiVersion,
    required this.requireAuth,
  });

  final String name;
  final String apiVersion;
  final bool requireAuth;

  factory AgentInfo.fromJson(Map<String, dynamic> json) => AgentInfo(
        name: json['name'] as String? ?? 'bunny',
        apiVersion: json['api_version'] as String? ?? 'v1',
        requireAuth: json['require_auth'] as bool? ?? true,
      );
}
