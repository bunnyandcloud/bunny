import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:web_socket_channel/io.dart';

import 'api.dart';

/// WebRTC video stream of the remote browser (CDP screencast on the agent).
class BrowserWebRtcService {
  RTCPeerConnection? _pc;
  RTCVideoRenderer? renderer;
  IOWebSocketChannel? _iceChannel;
  bool _connected = false;

  bool get isConnected => _connected;
  RTCVideoRenderer? get videoRenderer => renderer;

  Future<void> connect({
    required BunnyApi api,
    required String sessionId,
    required String browserId,
  }) async {
    await disconnect();
    final config = await api.webrtcConfig();
    if (!config.enabled) {
      debugPrint('bunny: browser WebRTC disabled');
      return;
    }

    renderer = RTCVideoRenderer();
    await renderer!.initialize();

    final iceServers = config.iceServers.map((s) {
      final urls = s['urls'];
      return {
        'urls': urls is List ? urls : [urls],
        if (s['username'] != null) 'username': s['username'],
        if (s['credential'] != null) 'credential': s['credential'],
      };
    }).toList();

    _pc = await createPeerConnection({'iceServers': iceServers});

    _pc!.onTrack = (event) {
      if (event.track.kind == 'video' && event.streams.isNotEmpty) {
        renderer!.srcObject = event.streams[0];
        _connected = true;
      }
    };

    _pc!.onIceCandidate = (c) async {
      if (c.candidate == null) return;
      try {
        await api.browserWebrtcCandidate(browserId, c.toMap());
      } catch (e) {
        debugPrint('bunny: browser ICE failed: $e');
      }
    };

    await _listenIce(api, sessionId);

    final offer = await _pc!.createOffer({
      'offerToReceiveVideo': true,
      'offerToReceiveAudio': false,
    });
    await _pc!.setLocalDescription(offer);

    final answer = await api.browserWebrtcOffer(
      browserId,
      SdpOffer(type: offer.type ?? 'offer', sdp: offer.sdp ?? ''),
    );
    await _pc!.setRemoteDescription(
      RTCSessionDescription(answer.sdp, answer.type),
    );
  }

  Future<void> _listenIce(BunnyApi api, String sessionId) async {
    final token = await api.getToken();
    final uri = Uri.parse(api.sessionRealtimeWsUrl(sessionId));
    _iceChannel = IOWebSocketChannel.connect(
      uri,
      headers: token != null ? {'Authorization': 'Bearer $token'} : null,
    );
    _iceChannel!.stream.listen((raw) {
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

  Future<void> disconnect() async {
    _connected = false;
    await _iceChannel?.sink.close();
    _iceChannel = null;
    await _pc?.close();
    _pc = null;
    await renderer?.dispose();
    renderer = null;
  }
}
