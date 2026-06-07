import 'package:flutter/material.dart';
import '../models/server_profile.dart';
import '../services/api.dart';
import '../services/server_store.dart';
import '../services/ssh_tunnel_service.dart';

class ConnectServerScreen extends StatefulWidget {
  const ConnectServerScreen({
    super.key,
    required this.profile,
    required this.store,
    required this.tunnel,
  });

  final ServerProfile profile;
  final ServerStore store;
  final SshTunnelService tunnel;

  @override
  State<ConnectServerScreen> createState() => _ConnectServerScreenState();
}

class _ConnectServerScreenState extends State<ConnectServerScreen> {
  TunnelStatus _status = TunnelStatus.disconnected;
  String? _message;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    widget.tunnel.states.listen((s) {
      if (mounted) {
        setState(() {
          _status = s.status;
          _message = s.message;
        });
      }
    });
    if (widget.tunnel.activeProfile?.id == widget.profile.id &&
        widget.tunnel.isConnected) {
      _status = TunnelStatus.connected;
      _message = 'Tunnel active';
    }
  }

  Future<void> _connect() async {
    setState(() => _busy = true);
    try {
      await widget.tunnel.connect(widget.profile);
      final ok = await widget.tunnel.verifyBunnyReachable(widget.profile);
      if (!ok) {
        throw Exception(
          'SSH connected but bunny agent not reachable at 127.0.0.1:${widget.profile.bunnyPort}. Is bunny start running on the server?',
        );
      }
      final info = await BunnyApi(
        baseUrl: widget.profile.localBunnyBaseUrl,
        profileId: widget.profile.id,
      ).agentInfo();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Connected to ${info.name} API ${info.apiVersion}')),
        );
        Navigator.pop(context, true);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(e.toString()), backgroundColor: Colors.red.shade800),
        );
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _disconnect() async {
    await widget.tunnel.disconnect();
    if (mounted) Navigator.pop(context, false);
  }

  @override
  Widget build(BuildContext context) {
    final p = widget.profile;
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        title: Text(p.name),
        backgroundColor: const Color(0xFF161B22),
      ),
      body: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              '${p.sshUsername}@${p.host}:${p.sshPort}',
              style: const TextStyle(color: Color(0xFF8B949E)),
            ),
            const SizedBox(height: 8),
            Text(
              'SSH tunnel: 127.0.0.1:${p.localForwardPort} → server 127.0.0.1:${p.bunnyPort}',
              style: const TextStyle(fontSize: 12, color: Color(0xFF8B949E)),
            ),
            const SizedBox(height: 24),
            _statusTile(),
            if (_message != null) ...[
              const SizedBox(height: 12),
              Text(_message!, style: const TextStyle(color: Color(0xFF8B949E), fontSize: 13)),
            ],
            const Spacer(),
            if (_status == TunnelStatus.connected)
              OutlinedButton(
                onPressed: _busy ? null : _disconnect,
                child: const Text('Disconnect'),
              )
            else
              FilledButton.icon(
                onPressed: _busy ? null : _connect,
                icon: _busy
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.vpn_key),
                label: Text(_busy ? 'Connecting…' : 'Connect SSH tunnel'),
              ),
          ],
        ),
      ),
    );
  }

  Widget _statusTile() {
    IconData icon;
    Color color;
    String label;
    switch (_status) {
      case TunnelStatus.connected:
        icon = Icons.check_circle;
        color = const Color(0xFF3FB950);
        label = 'Tunnel connected';
      case TunnelStatus.connecting:
        icon = Icons.hourglass_top;
        color = const Color(0xFF9498FF);
        label = 'Connecting…';
      case TunnelStatus.error:
        icon = Icons.error_outline;
        color = Colors.redAccent;
        label = 'Error';
      case TunnelStatus.disconnected:
        icon = Icons.link_off;
        color = const Color(0xFF8B949E);
        label = 'Not connected';
    }
    return Row(
      children: [
        Icon(icon, color: color),
        const SizedBox(width: 12),
        Text(label, style: TextStyle(fontSize: 16, color: color)),
      ],
    );
  }
}
