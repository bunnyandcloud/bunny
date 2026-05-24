import 'package:flutter/material.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:webview_flutter/webview_flutter.dart';
import '../models/server_profile.dart';
import '../services/api.dart';
import '../services/browser_webrtc_service.dart';
import '../services/push_service.dart';
import '../services/server_store.dart';
import '../services/ssh_tunnel_service.dart';
import '../services/webrtc_service.dart';
import 'terminal_screen.dart';

class SessionShell extends StatefulWidget {
  const SessionShell({
    super.key,
    required this.profile,
    required this.api,
    required this.tunnel,
    required this.onDisconnect,
    required this.store,
  });

  final ServerProfile profile;
  final BunnyApi api;
  final SshTunnelService tunnel;
  final ServerStore store;
  final VoidCallback onDisconnect;

  @override
  State<SessionShell> createState() => _SessionShellState();
}

class _SessionShellState extends State<SessionShell> {
  int _tab = 0;
  String? _sessionId;
  String? _terminalId;
  String? _browserId;
  String? _error;
  final _webrtc = WebRtcService();
  final _browserWebRtc = BrowserWebRtcService();
  bool _webrtcOk = false;
  bool _browserWebRtcOk = false;
  bool _pushOk = false;
  bool _browserUseWebView = false;
  bool _browserConnecting = false;

  @override
  void initState() {
    super.initState();
    _bootstrapSession();
  }

  @override
  void dispose() {
    _webrtc.disconnect();
    _browserWebRtc.disconnect();
    super.dispose();
  }

  Future<void> _bootstrapSession() async {
    try {
      final sessions = await widget.api.listSessions();
      String sessionId;
      if (sessions.isNotEmpty) {
        sessionId = (sessions.first as Map<String, dynamic>)['id'] as String;
      } else {
        final created = await widget.api.createSession(projectPath: '/');
        sessionId = created['id'] as String;
      }
      final t = await widget.api.createTerminal(sessionId, 'mobile');

      String? browserId;
      try {
        final browser = await widget.api.createBrowser(
          sessionId,
          targetUrl: 'http://127.0.0.1:3000',
        );
        browserId = browser['id'] as String?;
      } catch (_) {}

      var pushOk = false;
      try {
        pushOk = await PushService.instance.registerWithAgent(
          widget.api,
          widget.store,
        );
      } catch (_) {}

      var webrtcOk = false;
      try {
        await _webrtc.connect(
          api: widget.api,
          sessionId: sessionId,
          onRealtimeEvent: (_) {},
        );
        webrtcOk = _webrtc.isConnected;
      } catch (_) {}

      if (!mounted) return;
      setState(() {
        _sessionId = sessionId;
        _terminalId = t['id'] as String;
        _browserId = browserId;
        _pushOk = pushOk;
        _webrtcOk = webrtcOk;
      });
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _connectBrowserWebRtc() async {
    final sessionId = _sessionId;
    final browserId = _browserId;
    if (sessionId == null || browserId == null || _browserConnecting) return;
    setState(() => _browserConnecting = true);
    try {
      await _browserWebRtc.connect(
        api: widget.api,
        sessionId: sessionId,
        browserId: browserId,
      );
      if (mounted) {
        setState(() {
          _browserWebRtcOk = _browserWebRtc.isConnected;
          _browserUseWebView = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _browserWebRtcOk = false;
          _browserUseWebView = true;
        });
      }
    } finally {
      if (mounted) setState(() => _browserConnecting = false);
    }
  }

  Widget _browserPanel(String base, String sessionId) {
    if (_browserConnecting) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_browserWebRtcOk &&
        _browserWebRtc.videoRenderer != null &&
        !_browserUseWebView) {
      return RTCVideoView(
        _browserWebRtc.videoRenderer!,
        objectFit: RTCVideoViewObjectFit.RTCVideoViewObjectFitContain,
      );
    }
    return Stack(
      children: [
        WebViewWidget(
          controller: WebViewController()
            ..setJavaScriptMode(JavaScriptMode.unrestricted)
            ..loadRequest(Uri.parse('$base/s/$sessionId/ports/3000/')),
        ),
        Positioned(
          bottom: 16,
          left: 16,
          right: 16,
          child: FilledButton.icon(
            onPressed: _browserId == null ? null : _connectBrowserWebRtc,
            icon: const Icon(Icons.videocam),
            label: Text(
              _browserId == null
                  ? 'Browser stack unavailable'
                  : 'Stream via WebRTC',
            ),
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Scaffold(
        backgroundColor: const Color(0xFF0D1117),
        appBar: AppBar(title: Text(widget.profile.name)),
        body: Center(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Text(_error!, style: const TextStyle(color: Colors.redAccent)),
          ),
        ),
      );
    }

    if (_sessionId == null || _terminalId == null) {
      return Scaffold(
        backgroundColor: const Color(0xFF0D1117),
        appBar: AppBar(
          title: Text(widget.profile.name),
          backgroundColor: const Color(0xFF161B22),
        ),
        body: const Center(child: CircularProgressIndicator()),
      );
    }

    final base = widget.api.baseUrl;
    final sessionId = _sessionId!;

    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        title: Text(widget.profile.name),
        backgroundColor: const Color(0xFF161B22),
        actions: [
          IconButton(
            icon: const Icon(Icons.logout),
            tooltip: 'Disconnect',
            onPressed: () async {
              if (_browserId != null) {
                await widget.api.browserWebrtcStop(_browserId!);
              }
              await _browserWebRtc.disconnect();
              await _webrtc.disconnect();
              await widget.tunnel.disconnect();
              await widget.api.clearToken();
              if (context.mounted) widget.onDisconnect();
            },
          ),
        ],
      ),
      body: IndexedStack(
        index: _tab,
        children: [
          TerminalScreen(api: widget.api, terminalId: _terminalId!),
          _browserPanel(base, sessionId),
          const Center(
            child: Text(
              'Console / Network (read-only)',
              style: TextStyle(color: Color(0xFF8B949E)),
            ),
          ),
          const Center(
            child: Text(
              'Timeline',
              style: TextStyle(color: Color(0xFF8B949E)),
            ),
          ),
          Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  widget.tunnel.isConnected ? Icons.link : Icons.link_off,
                  color: widget.tunnel.isConnected
                      ? const Color(0xFF3FB950)
                      : const Color(0xFF8B949E),
                ),
                const SizedBox(height: 8),
                Text(
                  widget.tunnel.isConnected ? 'SSH tunnel active' : 'Tunnel disconnected',
                  style: const TextStyle(color: Color(0xFF8B949E)),
                ),
                Text(
                  widget.profile.localBunnyBaseUrl,
                  style: const TextStyle(color: Color(0xFF8B949E), fontSize: 12),
                ),
                const SizedBox(height: 16),
                _statusRow(
                  Icons.podcasts,
                  'Push',
                  _pushOk ? 'Registered' : 'Not configured',
                  _pushOk ? const Color(0xFF3FB950) : const Color(0xFF8B949E),
                ),
                const SizedBox(height: 8),
                _statusRow(
                  Icons.swap_horiz,
                  'WebRTC control',
                  _webrtcOk ? 'Data channel' : 'Unavailable',
                  _webrtcOk ? const Color(0xFF3FB950) : const Color(0xFF8B949E),
                ),
                const SizedBox(height: 8),
                _statusRow(
                  Icons.videocam,
                  'WebRTC browser',
                  _browserWebRtcOk
                      ? 'Video stream'
                      : (_browserId != null ? 'Tap Browser → Stream' : 'No browser'),
                  _browserWebRtcOk ? const Color(0xFF3FB950) : const Color(0xFF8B949E),
                ),
              ],
            ),
          ),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _tab,
        onDestinationSelected: (i) {
          setState(() => _tab = i);
          if (i == 1 && _browserId != null && !_browserWebRtcOk && !_browserConnecting) {
            _connectBrowserWebRtc();
          }
        },
        destinations: const [
          NavigationDestination(icon: Icon(Icons.terminal), label: 'Terminal'),
          NavigationDestination(icon: Icon(Icons.web), label: 'Browser'),
          NavigationDestination(icon: Icon(Icons.bug_report), label: 'Console'),
          NavigationDestination(icon: Icon(Icons.timeline), label: 'Timeline'),
          NavigationDestination(icon: Icon(Icons.info_outline), label: 'Status'),
        ],
      ),
    );
  }

  Widget _statusRow(IconData icon, String label, String value, Color color) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 8),
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: const TextStyle(fontWeight: FontWeight.w600)),
            Text(value, style: TextStyle(color: color, fontSize: 12)),
          ],
        ),
      ],
    );
  }
}
