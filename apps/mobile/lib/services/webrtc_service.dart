import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:web_socket_channel/io.dart';

import 'api.dart';

typedef WebRtcMessageHandler = void Function(Map<String, dynamic> message);

class WebRtcService {
  RTCPeerConnection? _pc;
  RTCDataChannel? _dc;
  IOWebSocketChannel? _realtimeChannel;
  bool _connected = false;

  bool get isConnected => _connected;
  RTCDataChannel? get dataChannel => _dc;

  Future<void> connect({
    required BunnyApi api,
    required String sessionId,
    void Function(Map<String, dynamic>)? onRealtimeEvent,
  }) async {
    await disconnect();
    final config = await api.webrtcConfig();
    if (!config.enabled) {
      debugPrint('bunny: WebRTC disabled on agent');
      return;
    }

    final iceServers = config.iceServers.map((s) {
      final urls = s['urls'];
      return {
        'urls': urls is List ? urls : [urls],
        if (s['username'] != null) 'username': s['username'],
        if (s['credential'] != null) 'credential': s['credential'],
      };
    }).toList();

    _pc = await createPeerConnection({'iceServers': iceServers});
    _dc = await _pc!.createDataChannel(
      'bunny',
      RTCDataChannelInit()..ordered = true,
    );
    _dc!.onMessage = (msg) {
      final text = msg.text;
      if (text.isEmpty) return;
      try {
        final decoded = jsonDecode(text) as Map<String, dynamic>;
        onRealtimeEvent?.call(decoded);
      } catch (_) {
        onRealtimeEvent?.call({'type': 'webrtc.message', 'data': text});
      }
    };
    _dc!.onDataChannelState = (state) {
      _connected = state == RTCDataChannelState.RTCDataChannelOpen;
      debugPrint('bunny: WebRTC DC state $state');
    };

    _pc!.onIceCandidate = (c) async {
      if (c.candidate == null) return;
      try {
        await api.webrtcCandidate(sessionId, c.toMap());
      } catch (e) {
        debugPrint('bunny: ICE send failed: $e');
      }
    };

    await _connectRealtimeForIce(api, sessionId);

    final offer = await _pc!.createOffer();
    await _pc!.setLocalDescription(offer);
    final answer = await api.webrtcOffer(
      sessionId,
      SdpOffer(type: offer.type ?? 'offer', sdp: offer.sdp ?? ''),
    );
    await _pc!.setRemoteDescription(
      RTCSessionDescription(answer.sdp, answer.type),
    );
  }

  Future<void> _connectRealtimeForIce(BunnyApi api, String sessionId) async {
    final token = await api.getToken();
    final uri = Uri.parse(api.sessionRealtimeWsUrl(sessionId));
    _realtimeChannel = IOWebSocketChannel.connect(
      uri,
      headers: token != null ? {'Authorization': 'Bearer $token'} : null,
    );
    _realtimeChannel!.stream.listen((raw) {
      try {
        final msg = jsonDecode(raw as String) as Map<String, dynamic>;
        if (msg['type'] == 'webrtc.ice' && msg['candidate'] != null) {
          final cand = msg['candidate'] as Map<String, dynamic>;
          _pc?.addCandidate(RTCIceCandidate(
            cand['candidate'] as String? ?? '',
            cand['sdpMid'] as String?,
            cand['sdpMLineIndex'] as int?,
          ));
        }
      } catch (_) {}
    });
  }

  void sendJson(Map<String, dynamic> payload) {
    final dc = _dc;
    if (dc == null || dc.state != RTCDataChannelState.RTCDataChannelOpen) {
      return;
    }
    dc.send(RTCDataChannelMessage(jsonEncode(payload)));
  }

  Future<void> disconnect() async {
    _connected = false;
    await _realtimeChannel?.sink.close();
    _realtimeChannel = null;
    await _dc?.close();
    _dc = null;
    await _pc?.close();
    _pc = null;
  }
}
